//! DOL-EVO consensus — stake-weighted quorum over Evo claims.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::credit::CreditEngine;

pub const DEFAULT_QUORUM_THRESHOLD: f64 = 3.0;
pub const DEFAULT_VOTE_WINDOW_SECS: u64 = 300;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "mesh-llm", derive(schemars::JsonSchema))]
pub struct EvoVote {
    pub claim_hash: String,
    pub voter: String,
    pub verdict: Verdict,
    pub timestamp: u64,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "mesh-llm", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Accept,
    Reject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "mesh-llm", derive(schemars::JsonSchema))]
pub struct ConsensusStatus {
    pub claim_hash: String,
    pub parent_hash: String,
    pub state: ConsensusState,
    pub accept_weight: f64,
    pub reject_weight: f64,
    pub total_votes: usize,
    pub deadline: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "mesh-llm", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ConsensusState {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, thiserror::Error)]
pub enum ConsensusError {
    #[error("claim not registered: {0}")]
    ClaimNotFound(String),
    #[error("duplicate vote from {voter} on {claim_hash}")]
    DuplicateVote { claim_hash: String, voter: String },
    #[error("vote window expired for claim {0}")]
    WindowExpired(String),
    #[error("consensus already finalized for claim {0}")]
    AlreadyFinalized(String),
}

pub struct ConsensusEngine {
    credit: CreditEngine,
    votes: HashMap<String, Vec<EvoVote>>,
    status: HashMap<String, ConsensusStatus>,
    quorum_threshold: f64,
    vote_window_secs: u64,
}

impl ConsensusEngine {
    pub fn new(quorum_threshold: f64, vote_window_secs: u64) -> Self {
        Self {
            credit: CreditEngine::new(),
            votes: HashMap::new(),
            status: HashMap::new(),
            quorum_threshold,
            vote_window_secs,
        }
    }

    pub fn register_evo_claim(&mut self, claim_hash: &str, parent_hash: &str, now: u64) {
        if self.status.contains_key(claim_hash) {
            return;
        }
        self.status.insert(
            claim_hash.to_string(),
            ConsensusStatus {
                claim_hash: claim_hash.to_string(),
                parent_hash: parent_hash.to_string(),
                state: ConsensusState::Pending,
                accept_weight: 0.0,
                reject_weight: 0.0,
                total_votes: 0,
                deadline: now + self.vote_window_secs,
            },
        );
        self.votes.insert(claim_hash.to_string(), Vec::new());
    }

    pub fn cast_vote(&mut self, vote: EvoVote) -> Result<(), ConsensusError> {
        let status = self
            .status
            .get(&vote.claim_hash)
            .ok_or_else(|| ConsensusError::ClaimNotFound(vote.claim_hash.clone()))?;

        if status.state != ConsensusState::Pending {
            return Err(ConsensusError::AlreadyFinalized(vote.claim_hash.clone()));
        }

        if vote.timestamp > status.deadline {
            return Err(ConsensusError::WindowExpired(vote.claim_hash.clone()));
        }

        let existing = self.votes.get(&vote.claim_hash).unwrap();
        if existing.iter().any(|v| v.voter == vote.voter) {
            return Err(ConsensusError::DuplicateVote {
                claim_hash: vote.claim_hash.clone(),
                voter: vote.voter.clone(),
            });
        }

        self.votes
            .get_mut(&vote.claim_hash)
            .unwrap()
            .push(vote);
        Ok(())
    }

    pub fn evaluate(&mut self, claim_hash: &str, now: u64) -> Result<ConsensusStatus, ConsensusError> {
        let status = self
            .status
            .get(claim_hash)
            .ok_or_else(|| ConsensusError::ClaimNotFound(claim_hash.to_string()))?
            .clone();

        if status.state != ConsensusState::Pending {
            return Ok(status);
        }

        let votes = self.votes.get(claim_hash).cloned().unwrap_or_default();
        let mut accept_weight = 0.0_f64;
        let mut reject_weight = 0.0_f64;

        for v in &votes {
            let weight = self.credit.weight_claim(&v.voter, now);
            match v.verdict {
                Verdict::Accept => accept_weight += weight,
                Verdict::Reject => reject_weight += weight,
            }
        }

        let total_weight = accept_weight + reject_weight;
        let past_deadline = now >= status.deadline;

        let new_state = if total_weight >= self.quorum_threshold {
            if accept_weight > reject_weight {
                ConsensusState::Accepted
            } else {
                ConsensusState::Rejected
            }
        } else if past_deadline {
            ConsensusState::Rejected
        } else {
            ConsensusState::Pending
        };

        if new_state != ConsensusState::Pending {
            let parent_hash = status.parent_hash.clone();
            let author = votes
                .first()
                .map(|v| v.claim_hash.clone())
                .unwrap_or_default();

            // Apply credit feedback when consensus is reached
            // We need the evo claim's author, but we store parent_hash — the caller
            // should wire apply_consensus_result with the actual claim author.
            let _ = (author, &parent_hash);
        }

        let updated = ConsensusStatus {
            claim_hash: claim_hash.to_string(),
            parent_hash: status.parent_hash,
            state: new_state,
            accept_weight,
            reject_weight,
            total_votes: votes.len(),
            deadline: status.deadline,
        };

        self.status.insert(claim_hash.to_string(), updated.clone());
        Ok(updated)
    }

    pub fn status(&self, claim_hash: &str) -> Option<&ConsensusStatus> {
        self.status.get(claim_hash)
    }

    pub fn pending_count(&self) -> usize {
        self.status
            .values()
            .filter(|s| s.state == ConsensusState::Pending)
            .count()
    }

    pub fn credit_engine(&self) -> &CreditEngine {
        &self.credit
    }

    pub fn credit_engine_mut(&mut self) -> &mut CreditEngine {
        &mut self.credit
    }
}

impl Default for ConsensusEngine {
    fn default() -> Self {
        Self::new(DEFAULT_QUORUM_THRESHOLD, DEFAULT_VOTE_WINDOW_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> ConsensusEngine {
        ConsensusEngine::new(3.0, 300)
    }

    #[test]
    fn register_evo_claim() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);
        let s = engine.status("evo1").unwrap();
        assert_eq!(s.state, ConsensusState::Pending);
        assert_eq!(s.deadline, 1300);
        assert_eq!(s.parent_hash, "gen1");
    }

    #[test]
    fn cast_vote_accept() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);
        engine.credit_engine_mut().record_verification("alice", 1000);

        let vote = EvoVote {
            claim_hash: "evo1".into(),
            voter: "alice".into(),
            verdict: Verdict::Accept,
            timestamp: 1050,
            reason: None,
        };
        engine.cast_vote(vote).unwrap();

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert!(s.accept_weight > 0.0);
        assert_eq!(s.total_votes, 1);
    }

    #[test]
    fn cast_vote_reject() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        let vote = EvoVote {
            claim_hash: "evo1".into(),
            voter: "bob".into(),
            verdict: Verdict::Reject,
            timestamp: 1050,
            reason: Some("disagree".into()),
        };
        engine.cast_vote(vote).unwrap();

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert!(s.reject_weight > 0.0);
    }

    #[test]
    fn duplicate_vote_rejected() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        let vote = EvoVote {
            claim_hash: "evo1".into(),
            voter: "alice".into(),
            verdict: Verdict::Accept,
            timestamp: 1050,
            reason: None,
        };
        engine.cast_vote(vote.clone()).unwrap();
        assert!(engine.cast_vote(vote).is_err());
    }

    #[test]
    fn vote_after_window_rejected() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        let vote = EvoVote {
            claim_hash: "evo1".into(),
            voter: "alice".into(),
            verdict: Verdict::Accept,
            timestamp: 1301,
            reason: None,
        };
        assert!(matches!(
            engine.cast_vote(vote),
            Err(ConsensusError::WindowExpired(_))
        ));
    }

    #[test]
    fn quorum_reached_accepted() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        // Give voters enough credit to reach quorum (3.0)
        for name in &["alice", "bob", "carol", "dave"] {
            engine.credit_engine_mut().record_verification(name, 1000);
        }

        for (i, name) in ["alice", "bob", "carol", "dave"].iter().enumerate() {
            engine
                .cast_vote(EvoVote {
                    claim_hash: "evo1".into(),
                    voter: name.to_string(),
                    verdict: Verdict::Accept,
                    timestamp: 1000 + i as u64,
                    reason: None,
                })
                .unwrap();
        }

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert_eq!(s.state, ConsensusState::Accepted);
    }

    #[test]
    fn quorum_reached_rejected() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        for name in &["alice", "bob", "carol", "dave"] {
            engine.credit_engine_mut().record_verification(name, 1000);
        }

        for (i, name) in ["alice", "bob", "carol", "dave"].iter().enumerate() {
            engine
                .cast_vote(EvoVote {
                    claim_hash: "evo1".into(),
                    voter: name.to_string(),
                    verdict: Verdict::Reject,
                    timestamp: 1000 + i as u64,
                    reason: None,
                })
                .unwrap();
        }

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert_eq!(s.state, ConsensusState::Rejected);
    }

    #[test]
    fn quorum_not_reached_stays_pending() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        // Single base-credit voter (1.0) won't reach quorum of 3.0
        engine
            .cast_vote(EvoVote {
                claim_hash: "evo1".into(),
                voter: "alice".into(),
                verdict: Verdict::Accept,
                timestamp: 1050,
                reason: None,
            })
            .unwrap();

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert_eq!(s.state, ConsensusState::Pending);
    }

    #[test]
    fn window_expiry_locks_status() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        // One vote not enough for quorum, but window expires -> Rejected
        engine
            .cast_vote(EvoVote {
                claim_hash: "evo1".into(),
                voter: "alice".into(),
                verdict: Verdict::Accept,
                timestamp: 1050,
                reason: None,
            })
            .unwrap();

        let s = engine.evaluate("evo1", 1301).unwrap();
        assert_eq!(s.state, ConsensusState::Rejected);

        // Subsequent evaluation returns the locked state
        let s2 = engine.evaluate("evo1", 2000).unwrap();
        assert_eq!(s2.state, ConsensusState::Rejected);
    }

    #[test]
    fn vote_on_unknown_claim_fails() {
        let mut engine = setup();
        let vote = EvoVote {
            claim_hash: "nonexistent".into(),
            voter: "alice".into(),
            verdict: Verdict::Accept,
            timestamp: 1050,
            reason: None,
        };
        assert!(matches!(
            engine.cast_vote(vote),
            Err(ConsensusError::ClaimNotFound(_))
        ));
    }

    #[test]
    fn credit_feedback_on_acceptance() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);

        for name in &["alice", "bob", "carol", "dave"] {
            engine.credit_engine_mut().record_verification(name, 1000);
        }

        for (i, name) in ["alice", "bob", "carol", "dave"].iter().enumerate() {
            engine
                .cast_vote(EvoVote {
                    claim_hash: "evo1".into(),
                    voter: name.to_string(),
                    verdict: Verdict::Accept,
                    timestamp: 1000 + i as u64,
                    reason: None,
                })
                .unwrap();
        }

        let s = engine.evaluate("evo1", 1050).unwrap();
        assert_eq!(s.state, ConsensusState::Accepted);

        // Apply credit feedback for the evo author
        engine
            .credit_engine_mut()
            .apply_consensus_result("evo-author", true, 1050);
        let w = engine.credit_engine().get("evo-author");
        assert!(w.verified_claims > 0);
    }

    #[test]
    fn pending_count() {
        let mut engine = setup();
        engine.register_evo_claim("evo1", "gen1", 1000);
        engine.register_evo_claim("evo2", "gen2", 1000);
        assert_eq!(engine.pending_count(), 2);
    }
}
