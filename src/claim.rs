//! Core claim types — DOL v0.8.0 keywords: gen, evo, docs.

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A single DOL claim. The `kind` discriminator serialises as one of the three
/// DOL v0.8.0 keywords: `gen`, `evo`, `docs`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DolClaim {
    Gen {
        author: String,
        ttl_secs: u64,
        body: serde_json::Value,
    },
    Evo {
        author: String,
        ttl_secs: u64,
        parent: String,
        body: serde_json::Value,
    },
    Docs {
        author: String,
        ttl_secs: u64,
        target: String,
        body: String,
    },
}

impl DolClaim {
    pub fn author(&self) -> &str {
        match self {
            DolClaim::Gen { author, .. }
            | DolClaim::Evo { author, .. }
            | DolClaim::Docs { author, .. } => author,
        }
    }

    pub fn ttl_secs(&self) -> u64 {
        match self {
            DolClaim::Gen { ttl_secs, .. }
            | DolClaim::Evo { ttl_secs, .. }
            | DolClaim::Docs { ttl_secs, .. } => *ttl_secs,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            DolClaim::Gen { .. } => "gen",
            DolClaim::Evo { .. } => "evo",
            DolClaim::Docs { .. } => "docs",
        }
    }
}

/// Compute a deterministic BLAKE3 content hash for a [`DolClaim`].
///
/// Round-trips through BTreeMap-backed structure for stable key order.
pub fn claim_hash(claim: &DolClaim) -> String {
    let value = serde_json::to_value(claim).expect("DolClaim always serialises");
    let canonical = canonicalise(&value);
    let bytes = serde_json::to_vec(&canonical).expect("canonical Value always serialises");
    blake3::hash(&bytes).to_hex().to_string()
}

fn canonicalise(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> =
                map.iter().map(|(k, v)| (k.clone(), canonicalise(v))).collect();
            serde_json::to_value(sorted).expect("BTreeMap always serialises")
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(canonicalise).collect())
        }
        other => other.clone(),
    }
}

/// A claim with an Ed25519 signature from its author.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedClaim {
    pub claim: DolClaim,
    pub hash: String,
    pub signature: Vec<u8>,
}

impl SignedClaim {
    pub fn sign(claim: DolClaim, signing_key: &SigningKey) -> Result<Self, serde_json::Error> {
        let hash = claim_hash(&claim);
        let canonical = serde_json::to_vec(&claim)?;
        let signature = signing_key.sign(&canonical);
        Ok(Self {
            claim,
            hash,
            signature: signature.to_bytes().to_vec(),
        })
    }

    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<bool, SignVerifyError> {
        let canonical = serde_json::to_vec(&self.claim)?;
        let sig = Signature::from_bytes(
            self.signature
                .as_slice()
                .try_into()
                .map_err(|_| SignVerifyError::InvalidSignatureLength)?,
        );
        Ok(verifying_key.verify(&canonical, &sig).is_ok())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignVerifyError {
    #[error("JSON serialisation error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid signature length")]
    InvalidSignatureLength,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use serde_json::json;

    #[test]
    fn claim_roundtrip() {
        let claim = DolClaim::Gen {
            author: "alice".into(),
            ttl_secs: 3600,
            body: json!({"module": "container.exists", "note": "test"}),
        };
        let serialised = serde_json::to_string(&claim).unwrap();
        assert!(serialised.contains(r#""kind":"gen""#));
        let deserialized: DolClaim = serde_json::from_str(&serialised).unwrap();
        assert_eq!(claim, deserialized);

        let evo = DolClaim::Evo {
            author: "bob".into(),
            ttl_secs: 1800,
            parent: "abc123".into(),
            body: json!({"delta": true}),
        };
        let evo_json = serde_json::to_string(&evo).unwrap();
        assert!(evo_json.contains(r#""kind":"evo""#));
        assert_eq!(evo, serde_json::from_str::<DolClaim>(&evo_json).unwrap());

        let docs = DolClaim::Docs {
            author: "carol".into(),
            ttl_secs: 7200,
            target: "def456".into(),
            body: "This explains the thing.".into(),
        };
        let docs_json = serde_json::to_string(&docs).unwrap();
        assert!(docs_json.contains(r#""kind":"docs""#));
        assert_eq!(docs, serde_json::from_str::<DolClaim>(&docs_json).unwrap());
    }

    #[test]
    fn reject_malformed() {
        let bad = r#"{"kind":"gen","author":"a"}"#;
        assert!(serde_json::from_str::<DolClaim>(bad).is_err());

        let unknown_kind = r#"{"kind":"mutate","author":"a","ttl_secs":60,"parent":"x","body":{}}"#;
        assert!(serde_json::from_str::<DolClaim>(unknown_kind).is_err());
    }

    #[test]
    fn content_hash_deterministic() {
        let claim = DolClaim::Gen {
            author: "alice".into(),
            ttl_secs: 3600,
            body: json!({"b": 2, "a": 1}),
        };
        let h1 = claim_hash(&claim);
        let h2 = claim_hash(&claim);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn content_hash_key_order_independent() {
        let claim_a = DolClaim::Gen {
            author: "alice".into(),
            ttl_secs: 3600,
            body: json!({"z": 1, "a": 2}),
        };
        let mut map = serde_json::Map::new();
        map.insert("a".into(), json!(2));
        map.insert("z".into(), json!(1));
        let claim_b = DolClaim::Gen {
            author: "alice".into(),
            ttl_secs: 3600,
            body: serde_json::Value::Object(map),
        };
        assert_eq!(claim_hash(&claim_a), claim_hash(&claim_b));
    }

    #[test]
    fn sign_and_verify() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        let claim = DolClaim::Gen {
            author: format!("{:x?}", verifying_key.as_bytes()),
            ttl_secs: 3600,
            body: json!({"test": true}),
        };

        let signed = SignedClaim::sign(claim, &signing_key).unwrap();
        assert!(signed.verify(&verifying_key).unwrap());
        assert!(!signed.hash.is_empty());
    }
}
