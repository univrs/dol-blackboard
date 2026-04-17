# CLAUDE.md — dol-blackboard

## What This Is
DOL Blackboard — Layer 3 of the Univrs Integration Stack.
Bridges mesh-llm's ephemeral gossip (`blackboard.v1`) with DOL's typed, persistent, credit-weighted claim system.

## Architecture
- `src/claim.rs` — Core types: ClaimType (6 variants), DolClaim, SignedClaim (Ed25519)
- `src/store.rs` — Persistence via redb: indexed by ID, timestamp, author
- `src/credit.rs` — Credit engine: reputation scoring with exponential decay

## Key Integration Points
- **mesh-llm upstream:** `mesh-llm/src/plugins/blackboard/mod.rs` — gossip channel `blackboard.v1`, BlackboardItem type
- **DOL upstream:** `univrs/dol` — ontology types, identity primitives
- **Consumer:** Layer 4 (DOL-EVO) reads weighted claims for consensus

## Commands
```bash
cargo build
cargo test
cargo clippy
```

## Next Steps (Priority Order)
1. Wire up mesh-llm plugin subscription — listen to `blackboard.v1`, parse typed claims
2. Bridge Ed25519 identity — DOL sovereign identity ↔ mesh-llm peer ID
3. Expose MCP tools — `dol_blackboard_query`, `dol_blackboard_assert`
4. Feed into DOL-EVO consensus layer
