# dol-blackboard

**DOL Blackboard — typed claims (gen/evo/docs) over mesh-llm gossip**

Layer 3 of the [Univrs Integration Stack](https://metalearn.org/integration/).

## What This Is

`dol-blackboard` bridges [mesh-llm](https://github.com/Mesh-LLM/mesh-llm) gossip channels with DOL v0.8.0's structured claim system:

1. **Typed claims** — `gen` (new knowledge), `evo` (evolving existing claims), `docs` (documenting claims)
2. **BLAKE3 hashing** — deterministic content-addressed claim IDs with canonicalised key order
3. **Ed25519 signatures** — every claim can be cryptographically signed by the author
4. **Persistence** — claims survive beyond gossip TTL via redb embedded store
5. **Credit weighting** — claims carry reputation weight with time-decay
6. **Ring buffer** — in-memory fast-access feed of recent claims (capacity 256)

## Architecture

```
┌─────────────────────────────────────────┐
│  Layer 6: MetaLearn Loop                │
│  Layer 5: Spirit / Ghost / Loa          │
│  Layer 4: DOL-EVO Consensus             │
├─────────────────────────────────────────┤
│  Layer 3: DOL Blackboard  ← THIS REPO  │
├─────────────────────────────────────────┤
│  Layer 2: mesh-llm plugins              │
│  Layer 1: mesh-llm substrate            │
└─────────────────────────────────────────┘
```

## Build

```bash
cargo build
```

## Test

```bash
cargo test
```

## mesh-llm Registration

Add to `~/.mesh-llm/config.toml` once `mesh_llm_plugin` is published:

```toml
[[plugin]]
name = "dol-blackboard"
version = "0.2.0"
path = "/path/to/dol-blackboard"

[plugin.mesh]
channels = ["dol.v1"]

[plugin.events]
subscribe = ["peer_up"]
```

## MCP Tools

| Tool | Input | Output |
|------|-------|--------|
| `dol_claim_post` | `DolClaim` JSON | BLAKE3 content hash |
| `dol_claim_feed` | optional `n` (default 50) | last N claims as JSON |

## Claim Types

```json
{"kind": "gen",  "author": "...", "ttl_secs": 3600, "body": {...}}
{"kind": "evo",  "author": "...", "ttl_secs": 3600, "parent": "hash", "body": {...}}
{"kind": "docs", "author": "...", "ttl_secs": 7200, "target": "hash", "body": "..."}
```

## Status

Scaffolding complete. Pending: mesh-llm plugin wiring (blocked on upstream crate publish).

## License

MIT OR Apache-2.0
