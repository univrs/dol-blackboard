//! Wire protocol — message types for the dol.v1 gossip channel.

use serde::{Deserialize, Serialize};

use crate::claim::DolClaim;
use crate::consensus::EvoVote;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DolMessage {
    ClaimPost { claim: DolClaim },
    Vote(EvoVote),
    SyncRequest,
    SyncDigest { hashes: Vec<String> },
    FetchRequest { hashes: Vec<String> },
    FetchResponse { claims: Vec<DolClaim> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::Verdict;
    use serde_json::json;

    #[test]
    fn claim_post_roundtrip() {
        let msg = DolMessage::ClaimPost {
            claim: DolClaim::Gen {
                author: "alice".into(),
                ttl_secs: 60,
                body: json!({"test": true}),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"claim_post""#));
        let decoded: DolMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, DolMessage::ClaimPost { .. }));
    }

    #[test]
    fn vote_roundtrip() {
        let msg = DolMessage::Vote(EvoVote {
            claim_hash: "abc123".into(),
            voter: "bob".into(),
            verdict: Verdict::Reject,
            timestamp: 1000,
            reason: Some("nope".into()),
        });
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"vote""#));
        let decoded: DolMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, DolMessage::Vote(_)));
    }

    #[test]
    fn sync_request_roundtrip() {
        let msg = DolMessage::SyncRequest;
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: DolMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, DolMessage::SyncRequest));
    }

    #[test]
    fn sync_digest_roundtrip() {
        let msg = DolMessage::SyncDigest {
            hashes: vec!["h1".into(), "h2".into()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: DolMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, DolMessage::SyncDigest { .. }));
    }

    #[test]
    fn tag_discrimination() {
        let json = r#"{"type":"fetch_request","hashes":["a","b"]}"#;
        let msg: DolMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, DolMessage::FetchRequest { .. }));

        let bad = r#"{"type":"unknown_type"}"#;
        assert!(serde_json::from_str::<DolMessage>(bad).is_err());
    }
}
