//! mesh-llm plugin registration — wires DolClaim types, consensus, and HTTP bindings.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mesh_llm_plugin::{mcp::tool, plugin, plugin_server_info, PluginContext, PluginMetadata};
use tokio::sync::Mutex;

use anyhow::Context as _;

use crate::claim::{claim_hash, DolClaim};
use crate::consensus::{ConsensusEngine, ConsensusState, ConsensusStatus, EvoVote, Verdict};
use crate::credit::CreditWeight;
use crate::protocol::DolMessage;
use crate::ring::ClaimRing;
use crate::{DOL_CHANNEL, VERSION};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// MCP input/output types
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ClaimPostInput {
    pub claim: DolClaim,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ClaimPostOutput {
    pub hash: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ClaimFeedInput {
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub accepted_only: bool,
}

fn default_feed_limit() -> usize {
    50
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ClaimFeedOutput {
    pub claims: Vec<DolClaim>,
    pub count: usize,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EvoVoteInput {
    pub claim_hash: String,
    pub verdict: Verdict,
    pub voter: String,
    pub reason: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct EvoStatusInput {
    pub claim_hash: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreditQueryInput {
    pub author: String,
}

// ---------------------------------------------------------------------------
// HTTP input/output types
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct FeedQuery {
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
    #[serde(default)]
    pub since: u64,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub accepted_only: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ClaimByHashQuery {
    pub hash: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct VoteBody {
    pub claim_hash: String,
    pub verdict: Verdict,
    pub voter: String,
    pub reason: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ConsensusQuery {
    pub hash: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreditQuery {
    pub author: String,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct StatusResponse {
    pub version: String,
    pub ring_size: usize,
    pub pending_votes: usize,
    pub channel: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct StreamQuery {
    #[serde(default)]
    pub kind: Option<String>,
}

// ---------------------------------------------------------------------------
// Plugin builder
// ---------------------------------------------------------------------------

pub fn build_plugin(
    ring: Arc<ClaimRing>,
    consensus: Arc<Mutex<ConsensusEngine>>,
) -> mesh_llm_plugin::SimplePlugin {
    let ring_post = ring.clone();
    let ring_feed = ring.clone();
    let ring_channel = ring.clone();
    let ring_http_feed = ring.clone();
    let ring_http_post = ring.clone();
    let ring_http_status = ring.clone();
    let ring_http_stream = ring.clone();

    let consensus_vote = consensus.clone();
    let consensus_status = consensus.clone();
    let consensus_channel = consensus.clone();
    let consensus_http_vote = consensus.clone();
    let consensus_http_status_query = consensus.clone();
    let consensus_http_credit = consensus.clone();
    let consensus_mcp_credit = consensus.clone();
    let consensus_post = consensus.clone();

    plugin! {
        metadata: PluginMetadata::new(
            "dol-blackboard",
            VERSION,
            plugin_server_info(
                "dol-blackboard",
                VERSION,
                "DOL Blackboard",
                "Typed DOL claims (gen/evo/docs) with consensus and HTTP bindings",
                None::<String>,
            ),
        ),

        mesh: [
            mesh_llm_plugin::mesh::channel(DOL_CHANNEL),
        ],

        events: [
            mesh_llm_plugin::events::peer_up(),
        ],

        mcp: [
            tool("dol_claim_post")
                .description("Post a DOL claim (gen/evo/docs), returns its BLAKE3 content hash")
                .input::<ClaimPostInput>()
                .handle(move |input: ClaimPostInput, ctx: &mut PluginContext<'_>| {
                    let ring = ring_post.clone();
                    let consensus = consensus_post.clone();
                    Box::pin(async move {
                        let hash = claim_hash(&input.claim);
                        if let Some(parent) = input.claim.parent_hash() {
                            let mut eng = consensus.lock().await;
                            eng.register_evo_claim(&hash, parent, now_secs());
                        }
                        ring.push(input.claim.clone()).await;
                        let msg = DolMessage::ClaimPost { claim: input.claim };
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.claim", &msg).await?;
                        Ok(ClaimPostOutput { hash })
                    })
                }),

            tool("dol_claim_feed")
                .description("Return the last N claims from the ring buffer (default 50)")
                .input::<ClaimFeedInput>()
                .handle(move |input: ClaimFeedInput, _ctx: &mut PluginContext<'_>| {
                    let ring = ring_feed.clone();
                    Box::pin(async move {
                        let mut claims = ring.last_n(input.limit).await;
                        if let Some(ref kind) = input.kind {
                            claims.retain(|c| c.kind_str() == kind.as_str());
                        }
                        let count = claims.len();
                        Ok(ClaimFeedOutput { claims, count })
                    })
                }),

            tool("dol_evo_vote")
                .description("Cast a vote (accept/reject) on an Evo claim")
                .input::<EvoVoteInput>()
                .handle(move |input: EvoVoteInput, ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_vote.clone();
                    Box::pin(async move {
                        let now = now_secs();
                        let vote = EvoVote {
                            claim_hash: input.claim_hash.clone(),
                            voter: input.voter.clone(),
                            verdict: input.verdict,
                            timestamp: now,
                            reason: input.reason,
                        };
                        let mut eng = consensus.lock().await;
                        eng.cast_vote(vote.clone()).context("cast_vote")?;
                        let status = eng.evaluate(&input.claim_hash, now).context("evaluate")?;
                        if status.state != ConsensusState::Pending {
                            let accepted = status.state == ConsensusState::Accepted;
                            eng.credit_engine_mut()
                                .apply_consensus_result(&input.voter, accepted, now);
                        }
                        drop(eng);
                        let msg = DolMessage::Vote(vote);
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.vote", &msg).await?;
                        Ok(status)
                    })
                }),

            tool("dol_evo_status")
                .description("Get consensus status for an Evo claim")
                .input::<EvoStatusInput>()
                .handle(move |input: EvoStatusInput, _ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_status.clone();
                    Box::pin(async move {
                        let now = now_secs();
                        let mut eng = consensus.lock().await;
                        let status = eng.evaluate(&input.claim_hash, now).context("evaluate")?;
                        Ok(status)
                    })
                }),

            tool("dol_credit_query")
                .description("Get credit weight for an author")
                .input::<CreditQueryInput>()
                .handle(move |input: CreditQueryInput, _ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_mcp_credit.clone();
                    Box::pin(async move {
                        let eng = consensus.lock().await;
                        let weight = eng.credit_engine().get(&input.author);
                        Ok(weight)
                    })
                }),
        ],

        http: [
            mesh_llm_plugin::http::get("/feed")
                .binding_id("feed")
                .description("Read recent DOL claims from the ring buffer.")
                .input::<FeedQuery>()
                .output::<ClaimFeedOutput>()
                .handle(move |input: FeedQuery, _ctx: &mut PluginContext<'_>| {
                    let ring = ring_http_feed.clone();
                    Box::pin(async move {
                        let mut claims = ring.last_n(input.limit).await;
                        if let Some(ref kind) = input.kind {
                            claims.retain(|c| c.kind_str() == kind.as_str());
                        }
                        let count = claims.len();
                        Ok(ClaimFeedOutput { claims, count })
                    })
                }),

            mesh_llm_plugin::http::post("/claim")
                .binding_id("claim_post")
                .description("Post a new DOL claim (gen/evo/docs).")
                .input::<ClaimPostInput>()
                .output::<ClaimPostOutput>()
                .handle(move |input: ClaimPostInput, ctx: &mut PluginContext<'_>| {
                    let ring = ring_http_post.clone();
                    Box::pin(async move {
                        let hash = claim_hash(&input.claim);
                        ring.push(input.claim.clone()).await;
                        let msg = DolMessage::ClaimPost { claim: input.claim };
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.claim", &msg).await?;
                        Ok(ClaimPostOutput { hash })
                    })
                }),

            mesh_llm_plugin::http::post("/vote")
                .binding_id("vote")
                .description("Cast a vote on an Evo claim.")
                .input::<VoteBody>()
                .output::<ConsensusStatus>()
                .handle(move |input: VoteBody, ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_http_vote.clone();
                    Box::pin(async move {
                        let now = now_secs();
                        let vote = EvoVote {
                            claim_hash: input.claim_hash.clone(),
                            voter: input.voter.clone(),
                            verdict: input.verdict,
                            timestamp: now,
                            reason: input.reason,
                        };
                        let mut eng = consensus.lock().await;
                        eng.cast_vote(vote.clone()).context("cast_vote")?;
                        let status = eng.evaluate(&input.claim_hash, now).context("evaluate")?;
                        if status.state != ConsensusState::Pending {
                            let accepted = status.state == ConsensusState::Accepted;
                            eng.credit_engine_mut()
                                .apply_consensus_result(&input.voter, accepted, now);
                        }
                        drop(eng);
                        let msg = DolMessage::Vote(vote);
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.vote", &msg).await?;
                        Ok(status)
                    })
                }),

            mesh_llm_plugin::http::get("/consensus")
                .binding_id("consensus")
                .description("Get consensus status for an Evo claim.")
                .input::<ConsensusQuery>()
                .output::<ConsensusStatus>()
                .handle(move |input: ConsensusQuery, _ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_http_status_query.clone();
                    Box::pin(async move {
                        let now = now_secs();
                        let mut eng = consensus.lock().await;
                        let status = eng.evaluate(&input.hash, now).context("evaluate")?;
                        Ok(status)
                    })
                }),

            mesh_llm_plugin::http::get("/credit")
                .binding_id("credit")
                .description("Get credit weight for an author.")
                .input::<CreditQuery>()
                .output::<CreditWeight>()
                .handle(move |input: CreditQuery, _ctx: &mut PluginContext<'_>| {
                    let consensus = consensus_http_credit.clone();
                    Box::pin(async move {
                        let eng = consensus.lock().await;
                        let weight = eng.credit_engine().get(&input.author);
                        Ok(weight)
                    })
                }),

            mesh_llm_plugin::http::get("/status")
                .binding_id("status")
                .description("Plugin health and statistics.")
                .input::<serde_json::Value>()
                .output::<StatusResponse>()
                .handle(move |_input: serde_json::Value, _ctx: &mut PluginContext<'_>| {
                    let ring = ring_http_status.clone();
                    Box::pin(async move {
                        let ring_size = ring.len().await;
                        Ok(StatusResponse {
                            version: VERSION.to_string(),
                            ring_size,
                            pending_votes: 0,
                            channel: DOL_CHANNEL.to_string(),
                        })
                    })
                }),

            mesh_llm_plugin::http::get("/stream")
                .binding_id("stream")
                .description("SSE stream of live claims.")
                .input::<StreamQuery>()
                .sse()
                .handle(move |_input: StreamQuery, _ctx: &mut PluginContext<'_>| {
                    let _ring = ring_http_stream.clone();
                    Box::pin(async move {
                        // SSE handler — the actual streaming is managed by the
                        // mesh-llm runtime once .sse() is declared. This handler
                        // returns the initial payload; live events flow via the
                        // broadcast channel subscriber wired at plugin init.
                        Ok(serde_json::json!({"status": "streaming"}))
                    })
                }),
        ],

        on_initialized: |_ctx: &mut PluginContext<'_>| {
            Box::pin(async move {
                tracing::info!("dol-blackboard v{} initialised on channel {}", VERSION, DOL_CHANNEL);
                Ok(())
            })
        },

        on_channel_message: move |message: mesh_llm_plugin::proto::ChannelMessage, ctx: &mut PluginContext<'_>| {
            let ring = ring_channel.clone();
            let consensus = consensus_channel.clone();
            Box::pin(async move {
                if message.channel != DOL_CHANNEL {
                    return Ok(());
                }

                // Try DolMessage envelope first, fall back to raw DolClaim for v0.2.0 compat
                match serde_json::from_slice::<DolMessage>(&message.body) {
                    Ok(DolMessage::ClaimPost { claim }) => {
                        let hash = claim_hash(&claim);
                        tracing::info!("received claim {hash} on {DOL_CHANNEL}");
                        if let Some(parent) = claim.parent_hash() {
                            let mut eng = consensus.lock().await;
                            eng.register_evo_claim(&hash, parent, now_secs());
                        }
                        ring.push(claim).await;
                    }
                    Ok(DolMessage::Vote(vote)) => {
                        tracing::info!("received vote from {} on {}", vote.voter, vote.claim_hash);
                        let now = now_secs();
                        let mut eng = consensus.lock().await;
                        if let Err(e) = eng.cast_vote(vote.clone()) {
                            tracing::warn!("vote rejected: {e}");
                        } else {
                            match eng.evaluate(&vote.claim_hash, now) {
                                Ok(status) if status.state != ConsensusState::Pending => {
                                    let accepted = status.state == ConsensusState::Accepted;
                                    eng.credit_engine_mut()
                                        .apply_consensus_result(&vote.voter, accepted, now);
                                    tracing::info!(
                                        "consensus reached on {}: {:?}",
                                        vote.claim_hash,
                                        status.state
                                    );
                                }
                                Ok(_) => {}
                                Err(e) => tracing::warn!("evaluate error: {e}"),
                            }
                        }
                    }
                    Ok(DolMessage::SyncRequest) => {
                        tracing::debug!("sync request received, sending digest");
                        let reply = crate::handle_sync_request(&ring).await;
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.sync", &reply).await?;
                    }
                    Ok(DolMessage::SyncDigest { hashes }) => {
                        tracing::debug!("sync digest received ({} hashes), computing missing", hashes.len());
                        let reply = crate::handle_sync_digest(&ring, hashes).await;
                        if let DolMessage::FetchRequest { ref hashes } = reply {
                            if !hashes.is_empty() {
                                tracing::debug!("requesting {} missing claims", hashes.len());
                                ctx.send_json_channel(DOL_CHANNEL, "", "dol.sync", &reply).await?;
                            }
                        }
                    }
                    Ok(DolMessage::FetchRequest { hashes }) => {
                        tracing::debug!("fetch request for {} claims", hashes.len());
                        let reply = crate::handle_fetch_request(&ring, hashes).await;
                        ctx.send_json_channel(DOL_CHANNEL, "", "dol.sync", &reply).await?;
                    }
                    Ok(DolMessage::FetchResponse { claims }) => {
                        let count = claims.len();
                        let mut ingested = 0usize;
                        for claim in claims {
                            let hash = claim_hash(&claim);
                            if !ring.contains_hash(&hash).await {
                                if let Some(parent) = claim.parent_hash() {
                                    let mut eng = consensus.lock().await;
                                    eng.register_evo_claim(&hash, parent, now_secs());
                                }
                                ring.push(claim).await;
                                ingested += 1;
                            }
                        }
                        tracing::info!("fetch response: {count} received, {ingested} new");
                    }
                    Err(_) => {
                        // Fall back to raw DolClaim for backwards compatibility
                        match serde_json::from_slice::<DolClaim>(&message.body) {
                            Ok(claim) => {
                                let hash = claim_hash(&claim);
                                tracing::info!("received legacy claim {hash} on {DOL_CHANNEL}");
                                if let Some(parent) = claim.parent_hash() {
                                    let mut eng = consensus.lock().await;
                                    eng.register_evo_claim(&hash, parent, now_secs());
                                }
                                ring.push(claim).await;
                            }
                            Err(e) => {
                                tracing::warn!("malformed message on {DOL_CHANNEL}: {e}");
                            }
                        }
                    }
                }
                Ok(())
            })
        },

        on_mesh_event: |event: mesh_llm_plugin::proto::MeshEvent, ctx: &mut PluginContext<'_>| {
            Box::pin(async move {
                if event.kind == mesh_llm_plugin::proto::mesh_event::Kind::PeerUp as i32 {
                    let peer_id = event.peer.map(|p| p.peer_id).unwrap_or_default();
                    let announce = DolClaim::Gen {
                        author: format!("dol-blackboard@{peer_id}"),
                        ttl_secs: 300,
                        body: serde_json::json!({
                            "type": "presence",
                            "version": VERSION,
                        }),
                    };
                    let msg = DolMessage::ClaimPost { claim: announce };
                    ctx.send_json_channel(DOL_CHANNEL, "", "dol.announce", &msg).await?;
                }
                Ok(())
            })
        },
    }
}
