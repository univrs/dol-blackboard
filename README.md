# dol-blackboard

**DOL Blackboard — typed, persistent, credit-weighted claims over mesh-llm gossip**

Layer 3 of the [Univrs Integration Stack](https://metalearn.org/integration/).

## What This Is

`dol-blackboard` extends [mesh-llm's ephemeral blackboard plugin](https://github.com/Mesh-LLM/mesh-llm/tree/main/mesh-llm/src/plugins/blackboard) with:

1. **Typed claims** — structured DOL ontology types instead of raw text
2. **Persistence** — claims survive beyond the 48hr gossip TTL
3. **Credit weighting** — claims carry stake/reputation weight from DOL identity
4. **Ed25519 signatures** — every claim is cryptographically signed by the author's sovereign identity

## Architecture

```
┌─────────────────────────────────────────┐
│  Layer 6: MetaLearn Loop                │  ← fitness landscapes, ontogenesis
│  Layer 5: Spirit / Ghost / Loa          │  ← agent runtimes
│  Layer 4: DOL-EVO Consensus             │  ← stake-weighted evolution
├─────────────────────────────────────────┤
│  Layer 3: DOL Blackboard  ← THIS REPO  │  ← typed claims, persistence, credit
├─────────────────────────────────────────┤
│  Layer 2: mesh-llm plugins              │  ← blackboard gossip, MCP tools
│  Layer 1: mesh-llm substrate            │  ← QUIC/iroh P2P, OpenAI API
└─────────────────────────────────────────┘
```

## Bridge Design

dol-blackboard sits between mesh-llm's gossip layer and DOL's ontology system:

```
mesh-llm blackboard (ephemeral gossip)
        │
        ▼
┌──────────────────────┐
│   dol-blackboard     │
│                      │
│  ┌────────────────┐  │
│  │ Claim Parser   │  │  ← text → typed DOL claim
│  │ Signature      │  │  ← Ed25519 sign/verify
│  │ Persistence    │  │  ← redb local store
│  │ Credit Engine  │  │  ← weight by identity stake
│  │ Query API      │  │  ← structured search over claims
│  └────────────────┘  │
│                      │
└──────────────────────┘
        │
        ▼
DOL ontology (univrs-dol types)
```

## Claim Types

DOL claims extend mesh-llm's free-text `BlackboardItem` with structured types:

| Type | Prefix | Description |
|------|--------|-------------|
| `Assertion` | `ASSERT:` | A factual claim with optional evidence |
| `Observation` | `OBS:` | Sensor/runtime observation |
| `Hypothesis` | `HYP:` | Testable prediction |
| `Refutation` | `REFUTE:` | Counter-evidence to an existing claim |
| `Commitment` | `COMMIT:` | Promise to perform an action |
| `Receipt` | `RECEIPT:` | Proof of completed action |

These map to DOL's ontological primitives and can be validated, weighted, and evolved by Layer 4 (DOL-EVO).

## Dependencies

| Crate | Source | Purpose |
|-------|--------|---------|
| `mesh-llm-plugin` | [Mesh-LLM/mesh-llm](https://github.com/Mesh-LLM/mesh-llm) | Plugin trait, gossip transport |
| `dol-core` | [univrs/dol](https://github.com/univrs/dol) | Ontology types, identity |
| `ed25519-dalek` | crates.io | Cryptographic signatures |
| `redb` | crates.io | Embedded persistent store |
| `serde` / `schemars` | crates.io | Serialization, JSON Schema |

## Status

🟡 **Scaffolding** — Core types defined, bridge protocol designed. Not yet connected to live mesh-llm gossip.

## Roadmap

- [x] Repo scaffold + architecture doc
- [x] Core claim types (DolClaim, ClaimType, SignedClaim)
- [x] Persistence layer (redb)
- [ ] mesh-llm plugin integration — subscribe to blackboard.v1 channel
- [ ] Ed25519 signing bridge — DOL identity → mesh-llm peer ID
- [ ] Credit engine — weight claims by author stake
- [ ] Query API — structured search over typed claims
- [ ] MCP tool exposure — dol_blackboard_query, dol_blackboard_assert
- [ ] DOL-EVO integration — feed claims into evolutionary consensus (Layer 4)

## License

MIT OR Apache-2.0
