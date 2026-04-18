//! Credit engine — weight claims by author identity stake.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const BASE_CREDIT: f64 = 1.0;
const CREDIT_HALF_LIFE: f64 = 7.0 * 24.0 * 3600.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "mesh-llm", derive(schemars::JsonSchema))]
pub struct CreditWeight {
    pub author: String,
    pub raw_score: f64,
    pub effective_score: f64,
    pub verified_claims: u64,
    pub refuted_claims: u64,
    pub last_active: u64,
}

#[derive(Default)]
pub struct CreditEngine {
    ledger: HashMap<String, CreditWeight>,
}

impl CreditEngine {
    pub fn new() -> Self {
        Self::default()
    }

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
        entry.raw_score = (entry.raw_score - 0.3).max(0.1);
        entry.last_active = now;
        entry.effective_score = Self::decay(entry.raw_score, entry.last_active, now);
    }

    pub fn ledger(&self) -> &HashMap<String, CreditWeight> {
        &self.ledger
    }

    pub fn apply_consensus_result(&mut self, author: &str, accepted: bool, now: u64) {
        if accepted {
            self.record_verification(author, now);
        } else {
            self.record_refutation(author, now);
        }
    }

    pub fn weight_claim(&self, author: &str, now: u64) -> f64 {
        let entry = self.get(author);
        Self::decay(entry.raw_score, entry.last_active, now)
    }

    fn decay(score: f64, last_active: u64, now: u64) -> f64 {
        let elapsed = (now.saturating_sub(last_active)) as f64;
        score * (2.0_f64).powf(-elapsed / CREDIT_HALF_LIFE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_credit() {
        let engine = CreditEngine::new();
        let w = engine.get("alice");
        assert_eq!(w.raw_score, BASE_CREDIT);
    }

    #[test]
    fn verification_boosts_credit() {
        let mut engine = CreditEngine::new();
        engine.record_verification("alice", 1000);
        engine.record_verification("alice", 1001);
        let w = engine.get("alice");
        assert_eq!(w.verified_claims, 2);
        assert!(w.raw_score > BASE_CREDIT);
    }

    #[test]
    fn refutation_reduces_credit() {
        let mut engine = CreditEngine::new();
        engine.record_verification("bob", 1000);
        engine.record_refutation("bob", 1001);
        let w = engine.get("bob");
        assert!(w.raw_score < BASE_CREDIT + 0.5);
        assert!(w.raw_score >= 0.1);
    }

    #[test]
    fn decay_over_time() {
        let now = 1_000_000;
        let week_later = now + (7 * 24 * 3600);
        let w = CreditEngine::decay(2.0, now, week_later);
        assert!((w - 1.0).abs() < 0.01);
    }

    #[test]
    fn apply_consensus_accepted() {
        let mut engine = CreditEngine::new();
        engine.apply_consensus_result("alice", true, 1000);
        let w = engine.get("alice");
        assert_eq!(w.verified_claims, 1);
        assert!(w.raw_score > BASE_CREDIT);
    }

    #[test]
    fn apply_consensus_rejected() {
        let mut engine = CreditEngine::new();
        engine.apply_consensus_result("bob", false, 1000);
        let w = engine.get("bob");
        assert_eq!(w.refuted_claims, 1);
        assert!(w.raw_score < BASE_CREDIT);
    }

    #[test]
    fn ledger_accessor() {
        let mut engine = CreditEngine::new();
        engine.record_verification("alice", 1000);
        engine.record_verification("bob", 1001);
        let ledger = engine.ledger();
        assert_eq!(ledger.len(), 2);
        assert!(ledger.contains_key("alice"));
        assert!(ledger.contains_key("bob"));
    }
}
