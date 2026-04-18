# CLAUDE.md — dol-blackboard

## What This Is
DOL Blackboard — typed claims (gen/evo/docs per DOL v0.8.0) over mesh-llm gossip.
Bridges mesh-llm's ephemeral gossip channels with DOL's structured claim system.

## Architecture
- `src/claim.rs` — DolClaim enum (Gen/Evo/Docs), BLAKE3 hashing, Ed25519 signing
- `src/ring.rs` — In-memory ring buffer (VecDeque, capacity 256) for fast feed
- `src/store.rs` — Persistence via redb: indexed by hash, timestamp, author
- `src/credit.rs` — Credit engine: reputation scoring with exponential decay
- `src/lib.rs` — Public API, MCP handler stubs, mesh-llm plugin skeleton

## DOL v0.8.0 Keywords
The three claim kinds are `gen`, `evo`, `docs`. Never use the deprecated `gene`, `evolves`, `exegesis`.

## Commands
```bash
cargo build
cargo test
cargo clippy
```

## Integration Points
- **mesh-llm:** plugin registration via `plugin!` macro (feature-gated, pending upstream publish)
- **mesh channel:** `dol.v1`
- **mesh events:** `peer_up` triggers presence announcement
- **MCP tools:** `dol_claim_post`, `dol_claim_feed`

## Next Steps
1. Wire up mesh-llm plugin once upstream publishes `mesh_llm_plugin` crate
2. Integrate credit engine with claim validation pipeline
3. Expose persistence through MCP tools
4. Feed into DOL-EVO consensus layer (Layer 4)
