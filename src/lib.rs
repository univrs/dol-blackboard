//! DOL Blackboard — typed claims (gen/evo/docs) over mesh-llm gossip.
//!
//! Bridges mesh-llm's gossip channels with DOL v0.8.0's structured claim
//! system. Provides: typed claims, BLAKE3 content hashing, Ed25519 signing,
//! redb persistence, credit-weighted reputation, and an in-memory ring buffer
//! for fast feed access.

pub mod claim;
pub mod consensus;
pub mod credit;
#[cfg(feature = "mesh-llm")]
pub mod plugin;
pub mod protocol;
pub mod ring;
pub mod store;

pub use claim::{claim_hash, DolClaim, SignedClaim};
pub use consensus::{ConsensusEngine, ConsensusState, ConsensusStatus, EvoVote, Verdict};
pub use credit::{CreditEngine, CreditWeight};
pub use protocol::DolMessage;
pub use ring::{ClaimRing, RING_CAPACITY};
pub use store::ClaimStore;

pub const DOL_CHANNEL: &str = "dol.v1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------------------
// MCP handler stubs (used by the mesh-llm plugin macro)
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum BlackboardError {
    #[error("JSON error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("store error: {0}")]
    Store(#[from] store::StoreError),
    #[error("consensus error: {0}")]
    Consensus(#[from] consensus::ConsensusError),
}

pub async fn handle_claim_post(
    ring: &ClaimRing,
    claim: DolClaim,
) -> Result<String, BlackboardError> {
    let hash = claim_hash(&claim);
    ring.push(claim).await;
    Ok(hash)
}

pub async fn handle_claim_feed(
    ring: &ClaimRing,
    n: Option<usize>,
) -> Result<String, BlackboardError> {
    let limit = n.unwrap_or(50);
    let claims = ring.last_n(limit).await;
    let json = serde_json::to_string_pretty(&claims)?;
    Ok(json)
}

pub async fn handle_vote_cast(
    engine: &tokio::sync::Mutex<ConsensusEngine>,
    vote: EvoVote,
    now: u64,
) -> Result<ConsensusStatus, BlackboardError> {
    let mut eng = engine.lock().await;
    eng.cast_vote(vote.clone())?;
    let status = eng.evaluate(&vote.claim_hash, now)?;
    if status.state != ConsensusState::Pending {
        let accepted = status.state == ConsensusState::Accepted;
        eng.credit_engine_mut()
            .apply_consensus_result(&vote.voter, accepted, now);
    }
    Ok(status)
}

pub async fn handle_consensus_status(
    engine: &tokio::sync::Mutex<ConsensusEngine>,
    claim_hash: &str,
    now: u64,
) -> Result<ConsensusStatus, BlackboardError> {
    let mut eng = engine.lock().await;
    let status = eng.evaluate(claim_hash, now)?;
    Ok(status)
}

pub fn handle_credit_query(
    engine: &ConsensusEngine,
    author: &str,
) -> CreditWeight {
    engine.credit_engine().get(author)
}

// Real plugin registration lives in `src/plugin.rs` behind the `mesh-llm` feature.
// Build with `cargo build --features mesh-llm` to compile the plugin module.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn handle_claim_post_returns_hash() {
        let ring = ClaimRing::new();
        let claim = DolClaim::Gen {
            author: "test".into(),
            ttl_secs: 60,
            body: json!({"hello": "world"}),
        };
        let expected_hash = claim_hash(&claim);
        let result = handle_claim_post(&ring, claim).await.unwrap();
        assert_eq!(result, expected_hash);
        assert_eq!(ring.len().await, 1);
    }

    #[tokio::test]
    async fn handle_claim_feed_returns_json() {
        let ring = ClaimRing::new();
        for i in 0..5 {
            ring.push(DolClaim::Gen {
                author: format!("a-{i}"),
                ttl_secs: 60,
                body: json!({"i": i}),
            })
            .await;
        }
        let json_str = handle_claim_feed(&ring, Some(3)).await.unwrap();
        let parsed: Vec<DolClaim> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 3);
    }

    #[tokio::test]
    #[ignore]
    async fn two_instance_channel() {
        if std::env::var("DOL_BB_INTEGRATION").unwrap_or_default() != "1" {
            return;
        }

        let ring_a = ClaimRing::new();
        let claim = DolClaim::Gen {
            author: "instance-a".into(),
            ttl_secs: 300,
            body: json!({"type": "presence", "version": VERSION}),
        };
        let hash = handle_claim_post(&ring_a, claim.clone()).await.unwrap();
        assert!(!hash.is_empty());

        let payload = serde_json::to_vec(&claim).unwrap();

        let ring_b = ClaimRing::new();
        let received: DolClaim = serde_json::from_slice(&payload).unwrap();
        ring_b.push(received).await;

        assert_eq!(ring_b.len().await, 1);
        let feed = ring_b.last_n(1).await;
        assert_eq!(claim_hash(&feed[0]), hash);
    }
}
