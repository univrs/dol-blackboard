//! Credit engine — weight claims by author identity stake.
//!
//! In DOL, not all claims are equal. Authors earn credit through:
//! - Verified assertions (claims confirmed by others)
//! - Successful predictions (hypotheses that proved true)
//! - Fulfilled commitments (promises kept, evidenced by receipts)
//!
//! Credit decays over time (half-life model) to prevent stale reputation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default credit for a new identity.
const BASE_CREDIT: f64 = 1.0;
/// Half-life of credit in seconds (7 days).
const CREDIT_HALF_LIFE: f64 = 7.0 * 24.0 * 3600.0;

/// Credit weight for a claim author.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreditWeight {
    /// Author's public key (hex).
    pub author: String,
    /// Raw credit score (sum of contributions).
    pub raw_score: f64,
    /// Time-decayed effective score.
    pub effective_score: f64,
    /// Number of verified claims.
    pub verified_claims: u64,
    /// Number of refuted claims.
    pub refuted_claims: u64,
    /// Last activity timestamp.
    pub last_active: u64,
}

/// In-memory credit ledger. Persisted via the ClaimStore.
pub struct CreditEngine {
    ledger: HashMap<String, CreditWeight>,
}

impl CreditEngine {
    pub fn new() -> Self {
        Self {
            ledger: HashMap::new(),
        }
    }

    /// Get or create a credit entry for an author.
    pub fn get(&self, author: &str) -> CreditWeight {
        self.ledger.get(author).cloned().unwrap_or(CreditWeight {
            author: author.to_string(),
            raw_score: BASE_CREDIT,
            effective_score: BASE_CREDIT,
            verified_claims: 0,
            refuted_claims: 0,
            last_active: 0,
        })
    }

    /// Record a verified claim — boost author credit.
    pub fn record_verification(&mut self, author: &str, now: u64) {
        let entry = self.ledger.entry(author.to_string()).or_insert(CreditWeight {
            author: author.to_string(),
            raw_score: BASE_CREDIT,
            effective_score: BASE_CREDIT,
            verified_claims: 0,
            refuted_claims: 0,
            last_active: now,
        });
        entry.verified_claims += 1;
        entry.raw_score += 0.5;
        entry.last_active = now;
        entry.effective_score = Self::decay(entry.raw_score, entry.last_active, now);
    }

    /// Record a refuted claim — reduce author credit.
    pub fn record_refutation(&mut self, author: &str, now: u64) {
        let entry = self.ledger.entry(author.to_string()).or_insert(CreditWeight {
            author: author.to_string(),
            raw_score: BASE_CREDIT,
            effective_score: BASE_CREDIT,
            verified_claims: 0,
            refuted_claims: 0,
            last_active: now,
        });
        entry.refuted_claims += 1;
        entry.raw_score = (entry.raw_score - 0.3).max(0.1); // Floor at 0.1
        entry.last_active = now;
        entry.effective_score = Self::decay(entry.raw_score, entry.last_active, now);
    }

    /// Compute weight for a claim given the current time.
    pub fn weight_claim(&self, author: &str, now: u64) -> f64 {
        let entry = self.get(author);
        Self::decay(entry.raw_score, entry.last_active, now)
    }

    /// Exponential decay: score * 2^(-elapsed/half_life).
    fn decay(score: f64, last_active: u64, now: u64) -> f64 {
        let elapsed = (now.saturating_sub(last_active)) as f64;
        score * (2.0_f64).powf(-elapsed / CREDIT_HALF_LIFE)
    }
}

impl Default for CreditEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_credit() {
        let engine = CreditEngine::new();
        let w = engine.get("alice");
        assert_eq!(w.raw_score, BASE_CREDIT);
    }

    #[test]
    fn test_verification_boosts_credit() {
        let mut engine = CreditEngine::new();
        engine.record_verification("alice", 1000);
        engine.record_verification("alice", 1001);
        let w = engine.get("alice");
        assert_eq!(w.verified_claims, 2);
        assert!(w.raw_score > BASE_CREDIT);
    }

    #[test]
    fn test_refutation_reduces_credit() {
        let mut engine = CreditEngine::new();
        engine.record_verification("bob", 1000);
        engine.record_refutation("bob", 1001);
        let w = engine.get("bob");
        assert!(w.raw_score < BASE_CREDIT + 0.5);
        assert!(w.raw_score >= 0.1);
    }

    #[test]
    fn test_decay_over_time() {
        let engine = CreditEngine::new();
        let now = 1_000_000;
        let week_later = now + (7 * 24 * 3600);
        // After one half-life, weight should be ~half.
        let w = CreditEngine::decay(2.0, now, week_later);
        assert!((w - 1.0).abs() < 0.01);
    }
}
