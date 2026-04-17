//! DOL Blackboard — typed, persistent, credit-weighted claims over mesh-llm gossip.
//!
//! Bridge between mesh-llm's ephemeral `blackboard.v1` gossip channel
//! and DOL's structured ontology claim system.

pub mod claim;
pub mod store;
pub mod credit;

pub use claim::{ClaimType, DolClaim, SignedClaim};
pub use store::ClaimStore;
pub use credit::CreditWeight;
