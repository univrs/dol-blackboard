//! Persistence layer — typed claims survive beyond mesh-llm's gossip TTL.

use crate::claim::SignedClaim;
use crate::consensus::{ConsensusStatus, EvoVote};
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::path::Path;

const CLAIMS: TableDefinition<&str, &[u8]> = TableDefinition::new("claims");
const BY_TIME: TableDefinition<&[u8], &str> = TableDefinition::new("by_time");
const BY_AUTHOR: TableDefinition<&str, &str> = TableDefinition::new("by_author");
const VOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("votes");
const CONSENSUS: TableDefinition<&str, &[u8]> = TableDefinition::new("consensus");

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
        {
            let _ = txn.open_table(VOTES)?;
        }
        {
            let _ = txn.open_table(CONSENSUS)?;
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

    pub fn put_vote(&self, vote: &EvoVote) -> Result<(), StoreError> {
        let key = format!("{}:{}", vote.claim_hash, vote.voter);
        let data = serde_json::to_vec(vote)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(VOTES)?;
            table.insert(key.as_str(), data.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_votes(&self, claim_hash: &str) -> Result<Vec<EvoVote>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(VOTES)?;
        let prefix = format!("{}:", claim_hash);
        let mut results = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            if key.value().starts_with(&prefix) {
                let vote: EvoVote = serde_json::from_slice(value.value())?;
                results.push(vote);
            }
        }
        Ok(results)
    }

    pub fn put_consensus(&self, status: &ConsensusStatus) -> Result<(), StoreError> {
        let data = serde_json::to_vec(status)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(CONSENSUS)?;
            table.insert(status.claim_hash.as_str(), data.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_consensus(&self, claim_hash: &str) -> Result<Option<ConsensusStatus>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CONSENSUS)?;
        match table.get(claim_hash)? {
            Some(data) => {
                let status: ConsensusStatus = serde_json::from_slice(data.value())?;
                Ok(Some(status))
            }
            None => Ok(None),
        }
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
    fn vote_persistence() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        let vote = EvoVote {
            claim_hash: "evo1".into(),
            voter: "alice".into(),
            verdict: crate::consensus::Verdict::Accept,
            timestamp: 1000,
            reason: None,
        };
        store.put_vote(&vote).unwrap();

        let votes = store.get_votes("evo1").unwrap();
        assert_eq!(votes.len(), 1);
        assert_eq!(votes[0].voter, "alice");
    }

    #[test]
    fn consensus_persistence() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        let status = ConsensusStatus {
            claim_hash: "evo1".into(),
            parent_hash: "gen1".into(),
            state: crate::consensus::ConsensusState::Accepted,
            accept_weight: 4.5,
            reject_weight: 0.5,
            total_votes: 5,
            deadline: 1300,
        };
        store.put_consensus(&status).unwrap();

        let retrieved = store.get_consensus("evo1").unwrap().unwrap();
        assert_eq!(retrieved.state, crate::consensus::ConsensusState::Accepted);
        assert_eq!(retrieved.total_votes, 5);
    }

    #[test]
    fn votes_for_different_claims() {
        let tmp = NamedTempFile::new().unwrap();
        let store = ClaimStore::open(tmp.path()).unwrap();

        for (hash, voter) in &[("evo1", "alice"), ("evo1", "bob"), ("evo2", "carol")] {
            store
                .put_vote(&EvoVote {
                    claim_hash: hash.to_string(),
                    voter: voter.to_string(),
                    verdict: crate::consensus::Verdict::Accept,
                    timestamp: 1000,
                    reason: None,
                })
                .unwrap();
        }

        assert_eq!(store.get_votes("evo1").unwrap().len(), 2);
        assert_eq!(store.get_votes("evo2").unwrap().len(), 1);
        assert_eq!(store.get_votes("evo3").unwrap().len(), 0);
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
