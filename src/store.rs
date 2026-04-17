//! Persistence layer — typed claims survive beyond mesh-llm's 48hr gossip TTL.
//!
//! Uses redb (embedded key-value store) for local persistence.
//! Claims are indexed by ID, timestamp, author, and type.

use crate::claim::{ClaimType, DolClaim, SignedClaim};
use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;

/// Claims table: claim_id → JSON-serialized SignedClaim
const CLAIMS: TableDefinition<&str, &[u8]> = TableDefinition::new("claims");
/// Index: timestamp (big-endian bytes) → claim_id
const BY_TIME: TableDefinition<&[u8], &str> = TableDefinition::new("by_time");
/// Index: author_pubkey → comma-separated claim_ids
const BY_AUTHOR: TableDefinition<&str, &str> = TableDefinition::new("by_author");

pub struct ClaimStore {
    db: Database,
}

impl ClaimStore {
    /// Open or create a claim store at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(path)?;

        // Ensure tables exist.
        let txn = db.begin_write()?;
        { let _ = txn.open_table(CLAIMS)?; }
        { let _ = txn.open_table(BY_TIME)?; }
        { let _ = txn.open_table(BY_AUTHOR)?; }
        txn.commit()?;

        Ok(Self { db })
    }

    /// Store a signed claim. Returns the claim ID.
    pub fn put(&self, signed: &SignedClaim) -> Result<String> {
        let id = &signed.claim.id;
        let data = serde_json::to_vec(signed)?;
        let ts = signed.claim.timestamp.to_be_bytes();

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
                .get(signed.claim.author.as_str())?
                .map(|v| v.value().to_string())
                .unwrap_or_default();
            let updated = if existing.is_empty() {
                id.clone()
            } else {
                format!("{},{}", existing, id)
            };
            by_author.insert(signed.claim.author.as_str(), updated.as_str())?;
        }
        txn.commit()?;

        Ok(id.clone())
    }

    /// Get a claim by ID.
    pub fn get(&self, id: &str) -> Result<Option<SignedClaim>> {
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

    /// Query claims by time range (unix seconds).
    pub fn since(&self, from_ts: u64, limit: usize) -> Result<Vec<SignedClaim>> {
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

    /// Query claims by author public key.
    pub fn by_author(&self, author: &str) -> Result<Vec<SignedClaim>> {
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

    /// Count total claims in the store.
    pub fn count(&self) -> Result<u64> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CLAIMS)?;
        Ok(table.len()?)
    }
}
