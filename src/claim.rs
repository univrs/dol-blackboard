//! Core claim types — structured DOL claims that extend mesh-llm's BlackboardItem.

use ed25519_dalek::{Signature, SigningKey, VerifyingKey, Signer, Verifier};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The six fundamental claim types in DOL ontology.
///
/// These extend mesh-llm's free-text BlackboardItem with structured semantics.
/// Each maps to a DOL ontological primitive that can be validated, weighted,
/// and evolved by the DOL-EVO consensus layer.
#[derive(Clone, Debug, PartialEq, Eq, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    /// A factual claim with optional evidence links.
    Assertion,
    /// A sensor/runtime observation — raw data, not interpretation.
    Observation,
    /// A testable prediction — must include falsification criteria.
    Hypothesis,
    /// Counter-evidence to an existing claim, referenced by ID.
    Refutation,
    /// A promise to perform an action by a deadline.
    Commitment,
    /// Proof of a completed action — references the original Commitment.
    Receipt,
}

impl ClaimType {
    /// Parse from mesh-llm blackboard text prefix convention.
    ///
    /// Maps: `ASSERT:`, `OBS:`, `HYP:`, `REFUTE:`, `COMMIT:`, `RECEIPT:`
    pub fn from_prefix(text: &str) -> Option<(Self, &str)> {
        let text = text.trim();
        let prefixes: &[(&str, ClaimType)] = &[
            ("ASSERT:", ClaimType::Assertion),
            ("OBS:", ClaimType::Observation),
            ("HYP:", ClaimType::Hypothesis),
            ("REFUTE:", ClaimType::Refutation),
            ("COMMIT:", ClaimType::Commitment),
            ("RECEIPT:", ClaimType::Receipt),
        ];
        for (prefix, claim_type) in prefixes {
            if text.starts_with(prefix) {
                return Some((claim_type.clone(), text[prefix.len()..].trim()));
            }
        }
        None
    }
}

/// A structured DOL claim — the unit of knowledge on the blackboard.
#[derive(Clone, Debug, JsonSchema, Serialize, Deserialize)]
pub struct DolClaim {
    /// Unique claim ID (SHA-256 of content + author + timestamp).
    pub id: String,
    /// The claim type.
    pub claim_type: ClaimType,
    /// Author's Ed25519 public key (hex-encoded).
    pub author: String,
    /// The claim body text.
    pub body: String,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// Optional: ID of the claim this references (for Refutation, Receipt).
    pub references: Option<String>,
    /// Optional: evidence URLs supporting this claim.
    pub evidence: Vec<String>,
    /// Optional: mesh-llm BlackboardItem ID this was parsed from.
    pub gossip_id: Option<u64>,
    /// Credit weight assigned by the credit engine.
    pub weight: f64,
}

impl DolClaim {
    /// Create a new claim, computing its content-addressed ID.
    pub fn new(
        claim_type: ClaimType,
        author: String,
        body: String,
        timestamp: u64,
    ) -> Self {
        let id = Self::compute_id(&author, &body, timestamp);
        Self {
            id,
            claim_type,
            author,
            body,
            timestamp,
            references: None,
            evidence: Vec::new(),
            gossip_id: None,
            weight: 1.0,
        }
    }

    /// Content-addressed ID: SHA-256(author || body || timestamp).
    fn compute_id(author: &str, body: &str, timestamp: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(author.as_bytes());
        hasher.update(body.as_bytes());
        hasher.update(timestamp.to_le_bytes());
        hex::encode(hasher.finalize())
    }

    /// Add a reference to another claim (for Refutation/Receipt).
    pub fn with_reference(mut self, ref_id: String) -> Self {
        self.references = Some(ref_id);
        self
    }

    /// Add evidence URLs.
    pub fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }

    /// Link to the original mesh-llm gossip item.
    pub fn from_gossip(mut self, gossip_id: u64) -> Self {
        self.gossip_id = Some(gossip_id);
        self
    }
}

/// A claim with an Ed25519 signature from its author.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedClaim {
    pub claim: DolClaim,
    /// Ed25519 signature over the canonical JSON of the claim.
    pub signature: Vec<u8>,
}

impl SignedClaim {
    /// Sign a claim with the author's private key.
    pub fn sign(claim: DolClaim, signing_key: &SigningKey) -> anyhow::Result<Self> {
        let canonical = serde_json::to_vec(&claim)?;
        let signature = signing_key.sign(&canonical);
        Ok(Self {
            claim,
            signature: signature.to_bytes().to_vec(),
        })
    }

    /// Verify the signature against the author's public key.
    pub fn verify(&self, verifying_key: &VerifyingKey) -> anyhow::Result<bool> {
        let canonical = serde_json::to_vec(&self.claim)?;
        let sig = Signature::from_bytes(
            self.signature.as_slice().try_into()
                .map_err(|_| anyhow::anyhow!("invalid signature length"))?
        );
        Ok(verifying_key.verify(&canonical, &sig).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_type_from_prefix() {
        let (ct, body) = ClaimType::from_prefix("ASSERT: the sky is blue").unwrap();
        assert_eq!(ct, ClaimType::Assertion);
        assert_eq!(body, "the sky is blue");

        let (ct, body) = ClaimType::from_prefix("OBS: temperature is 22C").unwrap();
        assert_eq!(ct, ClaimType::Observation);
        assert_eq!(body, "temperature is 22C");

        assert!(ClaimType::from_prefix("STATUS: working on stuff").is_none());
    }

    #[test]
    fn test_claim_id_is_deterministic() {
        let c1 = DolClaim::new(ClaimType::Assertion, "alice".into(), "hello".into(), 1000);
        let c2 = DolClaim::new(ClaimType::Assertion, "alice".into(), "hello".into(), 1000);
        assert_eq!(c1.id, c2.id);

        let c3 = DolClaim::new(ClaimType::Assertion, "bob".into(), "hello".into(), 1000);
        assert_ne!(c1.id, c3.id);
    }

    #[test]
    fn test_sign_and_verify() {
        use ed25519_dalek::SigningKey;
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let claim = DolClaim::new(
            ClaimType::Hypothesis,
            hex::encode(verifying_key.as_bytes()),
            "entropy increases with complexity".into(),
            1713400000,
        );

        let signed = SignedClaim::sign(claim, &signing_key).unwrap();
        assert!(signed.verify(&verifying_key).unwrap());
    }
}
