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
7. **DOL-EVO consensus** — stake-weighted quorum voting on Evo claims with configurable vote windows
8. **HTTP bindings** — REST endpoints for feed, claims, voting, consensus status, credit, and SSE streaming

## Architecture

```
┌─────────────────────────────────────────┐
│  Layer 6: MetaLearn Loop                │
│  Layer 5: Spirit / Ghost / Loa          │
│  Layer 4: DOL-EVO Consensus  ← v0.3.0  │
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
# 38 passed, 1 ignored (integration test behind DOL_BB_INTEGRATION=1)
```

## mesh-llm Registration

Add to `~/.mesh-llm/config.toml`:

```toml
[[plugin]]
name = "dol-blackboard"
version = "0.3.0"
path = "/path/to/dol-blackboard"

[plugin.mesh]
channels = ["dol.v1"]

[plugin.events]
subscribe = ["peer_up"]
```

## MCP Tools

| Tool | Description | Input | Output |
|------|-------------|-------|--------|
| `dol_claim_post` | Post a DOL claim | `DolClaim` JSON | BLAKE3 content hash |
| `dol_claim_feed` | Recent claims from ring buffer | `limit`, `kind`, `accepted_only` | Claim array |
| `dol_evo_vote` | Cast a vote on an Evo claim | `claim_hash`, `verdict`, `voter` | `ConsensusStatus` |
| `dol_evo_status` | Get consensus status | `claim_hash` | `ConsensusStatus` |
| `dol_credit_query` | Get credit weight for an author | `author` | `CreditWeight` |

## HTTP Endpoints

All mounted under `/api/plugins/dol-blackboard/`:

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/feed` | Recent claims (supports `?limit=`, `?kind=`, `?accepted_only=`) |
| `POST` | `/claim` | Post a new DOL claim |
| `POST` | `/vote` | Cast a vote on an Evo claim |
| `GET` | `/consensus?hash=` | Get consensus status for an Evo claim |
| `GET` | `/credit?author=` | Get credit weight for an author |
| `GET` | `/status` | Plugin health and statistics |
| `GET` | `/stream` | SSE live claim stream |

## Claim Types

```json
{"kind": "gen",  "author": "...", "ttl_secs": 3600, "body": {...}}
{"kind": "evo",  "author": "...", "ttl_secs": 3600, "parent": "hash", "body": {...}}
{"kind": "docs", "author": "...", "ttl_secs": 7200, "target": "hash", "body": "..."}
```

## Consensus Model

Evo claims go through stake-weighted quorum voting:

- **Vote window**: 300 seconds (configurable)
- **Quorum threshold**: 3.0 effective credit units (configurable)
- **States**: Pending -> Accepted / Rejected (monotonic, locked after deadline)
- **Credit feedback**: acceptance boosts author credit (+0.5), rejection penalises (-0.3, floor 0.1)

## Wire Protocol

Messages on `dol.v1` use the `DolMessage` envelope:

```json
{"type": "claim_post", "claim": {...}}
{"type": "vote", "claim_hash": "...", "voter": "...", "verdict": "accept", ...}
{"type": "sync_request"}
{"type": "sync_digest", "hashes": [...]}
{"type": "fetch_request", "hashes": [...]}
{"type": "fetch_response", "claims": [...]}
```

Backwards-compatible: legacy raw `DolClaim` messages are accepted as fallback.

## License

MIT OR Apache-2.0
