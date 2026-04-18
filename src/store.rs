//! Persistence layer — typed claims survive beyond mesh-llm's gossip TTL.

use crate::claim::SignedClaim;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::path::Path;

const CLAIMS: TableDefinition<&str, &[u8]> = TableDefinition::new("claims");
const BY_TIME: TableDefinition<&[u8], &str> = TableDefinition::new("by_time");
const BY_AUTHOR: TableDefinition<&str, &str> = TableDefinition::new("by_author");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] redb::DatabaseError),
    #[error("table error: {0}")]
    Table(#[from] redb::TableError),
    #[error("storage error: {0}")]
    Storage(#[from] redb::StorageError),
    #[error("commit error: {0}")]
    Commit(#[from] redb::CommitError),
    #[error("transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct ClaimStore {
    db: Database,
}

impl ClaimStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let db = Database::create(path)?;
        let txn = db.begin_write()?;
        {
            let _ = txn.open_table(CLAIMS)?;
        }
        {
            let _ = txn.open_table(BY_TIME)?;
        }
        {
            let _ = txn.open_table(BY_AUTHOR)?;
        }
        txn.commit()?;
        Ok(Self { db })
    }

    pub fn put(&self, signed: &SignedClaim, timestamp: u64) -> Result<String, StoreError> {
        let id = &signed.hash;
        let data = serde_json::to_vec(signed)?;
        let ts = timestamp.to_be_bytes();
        let author = signed.claim.author();

        let txn = self.db.begin_write()?;
        {
            let mut claims = txn.open_table(CLAIMS)?;
            claims.insert(id.as_str(), data.as_slice())?;
        }
        {
            let mut by_time = txn.open_table(BY_TIME)?;
            by_time.insert(ts.as_slice(), id.as_str())?;
        }
        {
            let mut by_author = txn.open_table(BY_AUTHOR)?;
            let existing = by_author
                .get(author)?
                .map(|v| v.value().to_string())
                .unwrap_or_default();
            let updated = if existing.is_empty() {
                id.clone()
            } else {
                format!("{},{}", existing, id)
            };
            by_author.insert(author, updated.as_str())?;
        }
        txn.commit()?;
        Ok(id.clone())
    }

    pub fn get(&self, id: &str) -> Result<Option<SignedClaim>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CLAIMS)?;
        match table.get(id)? {
            Some(data) => {
                let signed: SignedClaim = serde_json::from_slice(data.value())?;
                Ok(Some(signed))
            }
            None => Ok(None),
        }
    }

    pub fn since(&self, from_ts: u64, limit: usize) -> Result<Vec<SignedClaim>, StoreError> {
        let txn = self.db.begin_read()?;
        let by_time = txn.open_table(BY_TIME)?;
        let claims_table = txn.open_table(CLAIMS)?;

        let start = from_ts.to_be_bytes();
        let mut results = Vec::new();

        for entry in by_time.range(start.as_slice()..)? {
            if results.len() >= limit {
                break;
            }
            let (_, claim_id) = entry?;
            if let Some(data) = claims_table.get(claim_id.value())? {
                let signed: SignedClaim = serde_json::from_slice(data.value())?;
                results.push(signed);
            }
        }
        Ok(results)
    }

    pub fn by_author(&self, author: &str) -> Result<Vec<SignedClaim>, StoreError> {
        let txn = self.db.begin_read()?;
        let by_author = txn.open_table(BY_AUTHOR)?;
        let claims_table = txn.open_table(CLAIMS)?;

        let ids = match by_author.get(author)? {
            Some(v) => v.value().to_string(),
            None => return Ok(Vec::new()),
        };

        let mut results = Vec::new();
        for id in ids.split(',') {
            if let Some(data) = claims_table.get(id)? {
                let signed: SignedClaim = serde_json::from_slice(data.value())?;
                results.push(signed);
            }
        }
        Ok(results)
    }

    pub fn count(&self) -> Result<u64, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CLAIMS)?;
        Ok(table.len()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::DolClaim;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use serde_json::json;
    use tempfile::NamedTempFile;

    fn test_signed_claim(body_val: &str) -> SignedClaim {
        let signing_key = SigningKey::generate(&mut OsRng);
        let claim = DolClaim::Gen {
            author: "test-author".into(),
            ttl_secs: 3600,
            body: json!({ "data": body_val }),
        };
        SignedClaim::sign(claim, &signing_key).unwrap()
    }

    #[test]
    fn roundtrip_store() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        let signed = test_signed_claim("hello");
        let id = store.put(&signed, 1000).unwrap();

        let retrieved = store.get(&id).unwrap().unwrap();
        assert_eq!(retrieved.hash, signed.hash);
    }

    #[test]
    fn query_by_author() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        let s1 = test_signed_claim("one");
        let s2 = test_signed_claim("two");
        store.put(&s1, 1000).unwrap();
        store.put(&s2, 1001).unwrap();

        let results = store.by_author("test-author").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn query_since() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        let s1 = test_signed_claim("early");
        let s2 = test_signed_claim("late");
        store.put(&s1, 1000).unwrap();
        store.put(&s2, 2000).unwrap();

        let results = store.since(1500, 10).unwrap();
        assert_eq!(results.len(), 1);
    }
}
