//! mesh-llm plugin registration — wires DolClaim types into the mesh-llm plugin API.

use std::sync::Arc;

use mesh_llm_plugin::{mcp::tool, plugin, plugin_server_info, PluginContext, PluginMetadata};

use crate::claim::{claim_hash, DolClaim};
use crate::ring::ClaimRing;
use crate::{DOL_CHANNEL, VERSION};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ClaimPostInput {
    pub claim: DolClaim,
}

#[derive(serde::Serialize)]
pub struct ClaimPostOutput {
    pub hash: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ClaimFeedInput {
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
}

fn default_feed_limit() -> usize {
    50
}

#[derive(serde::Serialize)]
pub struct ClaimFeedOutput {
    pub claims: Vec<DolClaim>,
    pub count: usize,
}

pub fn build_plugin(ring: Arc<ClaimRing>) -> mesh_llm_plugin::SimplePlugin {
    let ring_post = ring.clone();
    let ring_feed = ring.clone();
    let ring_channel = ring.clone();

    plugin! {
        metadata: PluginMetadata::new(
            "dol-blackboard",
            VERSION,
            plugin_server_info(
                "dol-blackboard",
                VERSION,
                "DOL Blackboard",
                "Typed DOL claims (gen/evo/docs) over mesh-llm gossip",
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
                .handle(move |input: ClaimPostInput, _ctx: &mut PluginContext<'_>| {
                    let ring = ring_post.clone();
                    Box::pin(async move {
                        let hash = claim_hash(&input.claim);
                        ring.push(input.claim).await;
                        Ok(ClaimPostOutput { hash })
                    })
                }),

            tool("dol_claim_feed")
                .description("Return the last N claims from the ring buffer (default 50)")
                .input::<ClaimFeedInput>()
                .handle(move |input: ClaimFeedInput, _ctx: &mut PluginContext<'_>| {
                    let ring = ring_feed.clone();
                    Box::pin(async move {
                        let claims = ring.last_n(input.limit).await;
                        let count = claims.len();
                        Ok(ClaimFeedOutput { claims, count })
                    })
                }),
        ],

        on_initialized: |_ctx: &mut PluginContext<'_>| {
            Box::pin(async move {
                tracing::info!("dol-blackboard v{} initialised on channel {}", VERSION, DOL_CHANNEL);
                Ok(())
            })
        },

        on_channel_message: move |message: mesh_llm_plugin::proto::ChannelMessage, _ctx: &mut PluginContext<'_>| {
            let ring = ring_channel.clone();
            Box::pin(async move {
                if message.channel == DOL_CHANNEL {
                    match serde_json::from_slice::<DolClaim>(&message.body) {
                        Ok(claim) => {
                            let hash = claim_hash(&claim);
                            tracing::info!("received claim {hash} on {DOL_CHANNEL}");
                            ring.push(claim).await;
                        }
                        Err(e) => {
                            tracing::warn!("malformed claim on {DOL_CHANNEL}: {e}");
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
                    ctx.send_json_channel(DOL_CHANNEL, "", "dol.announce", &announce).await?;
                }
                Ok(())
            })
        },
    }
}
