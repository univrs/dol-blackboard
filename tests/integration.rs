//! Integration tests for the full consensus flow and store round-trips.

use dol_blackboard::*;
use serde_json::json;
use tokio::sync::Mutex;

#[tokio::test]
async fn full_consensus_flow() {
    let ring = ClaimRing::new();
    let engine = Mutex::new(ConsensusEngine::default());

    // 1. Post a Gen claim (the parent)
    let gen_claim = DolClaim::Gen {
        author: "alice".into(),
        ttl_secs: 3600,
        body: json!({"module": "container.exists"}),
    };
    let gen_hash = handle_claim_post(&ring, gen_claim).await.unwrap();
    assert!(!gen_hash.is_empty());

    // 2. Post an Evo claim referencing the Gen
    let evo_claim = DolClaim::Evo {
        author: "bob".into(),
        ttl_secs: 1800,
        parent: gen_hash.clone(),
        body: json!({"delta": "add monitoring"}),
    };
    let evo_hash = claim_hash(&evo_claim);
    handle_claim_post(&ring, evo_claim).await.unwrap();

    // 3. Register the evo claim for consensus
    {
        let mut eng = engine.lock().await;
        eng.register_evo_claim(&evo_hash, &gen_hash, 1000);
    }

    // 4. Give voters enough credit to reach quorum (3.0)
    {
        let mut eng = engine.lock().await;
        for name in &["voter1", "voter2", "voter3", "voter4"] {
            eng.credit_engine_mut().record_verification(name, 1000);
        }
    }

    // 5. Cast votes — first a reject, then accepts to reach quorum
    for (i, (voter, verdict)) in [
        ("voter1", Verdict::Reject),
        ("voter2", Verdict::Accept),
        ("voter3", Verdict::Accept),
        ("voter4", Verdict::Accept),
    ]
    .iter()
    .enumerate()
    {
        let vote = EvoVote {
            claim_hash: evo_hash.clone(),
            voter: voter.to_string(),
            verdict: verdict.clone(),
            timestamp: 1000 + i as u64,
            reason: None,
        };
        let result = handle_vote_cast(&engine, vote, 1000 + i as u64).await;
        // The last vote that triggers quorum finalizes; subsequent votes would
        // get AlreadyFinalized — but we only cast 4, which is exactly enough.
        if result.is_err() {
            // Consensus already locked from a previous vote evaluation
            break;
        }
    }

    // 6. Evaluate — should be Accepted (3 accept vs 1 reject, quorum met)
    let status = handle_consensus_status(&engine, &evo_hash, 1050)
        .await
        .unwrap();
    assert_eq!(status.state, ConsensusState::Accepted);
    assert!(status.accept_weight > status.reject_weight);

    // 7. Verify credit was updated for voters
    let eng = engine.lock().await;
    let w = eng.credit_engine().get("voter1");
    assert!(w.verified_claims >= 1);

    // 8. Ring buffer has both claims
    assert_eq!(ring.len().await, 2);
    let feed = ring.last_n(10).await;
    assert_eq!(feed.len(), 2);
}

#[tokio::test]
async fn consensus_rejection_flow() {
    let engine = Mutex::new(ConsensusEngine::default());

    {
        let mut eng = engine.lock().await;
        eng.register_evo_claim("evo-reject", "gen-parent", 1000);
        for name in &["alice", "bob", "carol"] {
            eng.credit_engine_mut().record_verification(name, 1000);
        }
    }

    // All voters reject
    for (i, voter) in ["alice", "bob", "carol"].iter().enumerate() {
        let vote = EvoVote {
            claim_hash: "evo-reject".into(),
            voter: voter.to_string(),
            verdict: Verdict::Reject,
            timestamp: 1000 + i as u64,
            reason: Some("not viable".into()),
        };
        handle_vote_cast(&engine, vote, 1000 + i as u64)
            .await
            .unwrap();
    }

    let status = handle_consensus_status(&engine, "evo-reject", 1050)
        .await
        .unwrap();
    assert_eq!(status.state, ConsensusState::Rejected);
}

#[tokio::test]
async fn store_claim_and_vote_roundtrip() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let store = ClaimStore::open(tmp.path()).unwrap();

    // Store a signed claim
    let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    let claim = DolClaim::Gen {
        author: "test-author".into(),
        ttl_secs: 3600,
        body: json!({"data": "hello"}),
    };
    let signed = SignedClaim::sign(claim, &signing_key).unwrap();
    let hash = store.put(&signed, 1000).unwrap();

    // Retrieve it
    let retrieved = store.get(&hash).unwrap().unwrap();
    assert_eq!(retrieved.hash, signed.hash);

    // Store votes for an evo claim
    for voter in &["alice", "bob"] {
        let vote = EvoVote {
            claim_hash: "evo-test".into(),
            voter: voter.to_string(),
            verdict: Verdict::Accept,
            timestamp: 1000,
            reason: None,
        };
        store.put_vote(&vote).unwrap();
    }
    assert_eq!(store.get_votes("evo-test").unwrap().len(), 2);

    // Store and retrieve consensus status
    let status = ConsensusStatus {
        claim_hash: "evo-test".into(),
        parent_hash: hash.clone(),
        state: ConsensusState::Accepted,
        accept_weight: 3.5,
        reject_weight: 0.0,
        total_votes: 2,
        deadline: 1300,
    };
    store.put_consensus(&status).unwrap();

    let retrieved_status = store.get_consensus("evo-test").unwrap().unwrap();
    assert_eq!(retrieved_status.state, ConsensusState::Accepted);
    assert_eq!(retrieved_status.total_votes, 2);
}

#[tokio::test]
async fn wire_protocol_roundtrip() {
    use dol_blackboard::DolMessage;

    let claim = DolClaim::Gen {
        author: "alice".into(),
        ttl_secs: 60,
        body: json!({"test": true}),
    };
    let msg = DolMessage::ClaimPost {
        claim: claim.clone(),
    };

    // Serialize as if sending over gossip
    let bytes = serde_json::to_vec(&msg).unwrap();

    // Deserialize as if receiving
    let decoded: DolMessage = serde_json::from_slice(&bytes).unwrap();
    match decoded {
        DolMessage::ClaimPost { claim: received } => {
            assert_eq!(claim_hash(&received), claim_hash(&claim));
        }
        _ => panic!("expected ClaimPost"),
    }

    // Vote message round-trip
    let vote_msg = DolMessage::Vote(EvoVote {
        claim_hash: "abc".into(),
        voter: "bob".into(),
        verdict: Verdict::Accept,
        timestamp: 1000,
        reason: None,
    });
    let bytes = serde_json::to_vec(&vote_msg).unwrap();
    let decoded: DolMessage = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(decoded, DolMessage::Vote(_)));

    // Legacy fallback: raw DolClaim should still parse
    let raw_bytes = serde_json::to_vec(&claim).unwrap();
    assert!(serde_json::from_slice::<DolMessage>(&raw_bytes).is_err());
    let fallback: DolClaim = serde_json::from_slice(&raw_bytes).unwrap();
    assert_eq!(claim_hash(&fallback), claim_hash(&claim));
}

#[tokio::test]
async fn two_peer_sync_flow() {
    use dol_blackboard::{
        handle_fetch_request, handle_fetch_response, handle_sync_digest, handle_sync_request,
        DolMessage,
    };

    // Peer A has 3 claims
    let ring_a = ClaimRing::new();
    for i in 0..3 {
        ring_a
            .push(DolClaim::Gen {
                author: format!("peer-a-{i}"),
                ttl_secs: 60,
                body: json!({"i": i}),
            })
            .await;
    }

    // Peer B has 1 claim (different from A's)
    let ring_b = ClaimRing::new();
    ring_b
        .push(DolClaim::Gen {
            author: "peer-b-0".into(),
            ttl_secs: 60,
            body: json!({"b": true}),
        })
        .await;

    // Step 1: B sends SyncRequest, A responds with digest
    let digest = handle_sync_request(&ring_a).await;
    let a_hashes = match &digest {
        DolMessage::SyncDigest { hashes } => {
            assert_eq!(hashes.len(), 3);
            hashes.clone()
        }
        _ => panic!("expected SyncDigest"),
    };

    // Step 2: B computes which of A's claims it's missing
    let fetch_req = handle_sync_digest(&ring_b, a_hashes).await;
    let missing = match &fetch_req {
        DolMessage::FetchRequest { hashes } => {
            assert_eq!(hashes.len(), 3, "B is missing all 3 of A's claims");
            hashes.clone()
        }
        _ => panic!("expected FetchRequest"),
    };

    // Step 3: A sends the requested claims
    let fetch_resp = handle_fetch_request(&ring_a, missing).await;
    let claims = match fetch_resp {
        DolMessage::FetchResponse { claims } => {
            assert_eq!(claims.len(), 3);
            claims
        }
        _ => panic!("expected FetchResponse"),
    };

    // Step 4: B ingests the claims
    let ingested = handle_fetch_response(&ring_b, claims).await;
    assert_eq!(ingested.len(), 3);
    assert_eq!(ring_b.len().await, 4); // 1 original + 3 synced

    // Step 5: A second sync should find nothing missing
    let digest2 = handle_sync_request(&ring_a).await;
    let a_hashes2 = match digest2 {
        DolMessage::SyncDigest { hashes } => hashes,
        _ => panic!("expected SyncDigest"),
    };
    let fetch_req2 = handle_sync_digest(&ring_b, a_hashes2).await;
    match fetch_req2 {
        DolMessage::FetchRequest { hashes } => {
            assert!(hashes.is_empty(), "B should have all of A's claims now");
        }
        _ => panic!("expected FetchRequest"),
    }
}
