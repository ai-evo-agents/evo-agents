mod event_handler;
mod gateway_client;
mod health_check;
mod skill_engine;
mod soul;

use anyhow::{Context, Result, bail};
use evo_common::{logging::init_logging, messages::events};
use gateway_client::GatewayClient;
use rust_socketio::{Payload, asynchronous::ClientBuilder};
use serde_json::json;
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};
use tracing::{error, info, warn};

// ─── Entry point ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // First argument: path to the agent folder (contains soul.md, skills/, etc.)
    let agent_folder = std::env::args()
        .nth(1)
        .unwrap_or_else(|| std::env::var("AGENT_FOLDER").unwrap_or_else(|_| ".".to_string()));

    let agent_dir = PathBuf::from(&agent_folder);

    if !agent_dir.exists() {
        bail!("Agent folder does not exist: {}", agent_dir.display());
    }

    // Load soul.md to determine this runner's identity
    let soul = soul::load_soul(&agent_dir)
        .with_context(|| format!("Failed to load soul from {}", agent_dir.display()))?;

    // Init logging using role as component name (→ logs/<role>.log)
    let _log_guard = init_logging(&soul.role);

    info!(
        agent_id = %soul.agent_id,
        role     = %soul.role,
        folder   = %agent_dir.display(),
        behavior_len = soul.behavior.len(),
        "runner starting"
    );

    // Load available skills
    let skills = skill_engine::load_skills(&agent_dir);
    info!(skills = skills.len(), "skills loaded");

    // King address (Socket.IO server)
    let king_address =
        std::env::var("KING_ADDRESS").unwrap_or_else(|_| "http://localhost:3000".to_string());

    // Gateway address (LLM proxy)
    let gateway_address =
        std::env::var("GATEWAY_ADDRESS").unwrap_or_else(|_| "http://localhost:8080".to_string());

    info!(king = %king_address, gateway = %gateway_address, "connecting to king");

    // Create gateway client for LLM calls
    let gateway = Arc::new(
        GatewayClient::new(&gateway_address)
            .context("Failed to create gateway client")?,
    );

    run_client(&soul, &king_address, &skills, &gateway).await?;

    Ok(())
}

// ─── Socket.IO client loop ────────────────────────────────────────────────────

async fn run_client(
    soul: &soul::Soul,
    king_address: &str,
    skills: &[skill_engine::LoadedSkill],
    gateway: &Arc<GatewayClient>,
) -> Result<()> {
    let agent_id = soul.agent_id.clone();
    let role = soul.role.clone();

    // Build capabilities from skill manifests (deduplicated)
    let capabilities: Vec<String> = skills
        .iter()
        .flat_map(|s| s.manifest.capabilities.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();

    // Clone identifiers for each closure
    let (id_cmd, role_cmd) = (agent_id.clone(), role.clone());

    // Clones for pipeline handler (needs gateway + soul + skills)
    let soul_pipe = soul.clone();
    let gateway_pipe = Arc::clone(gateway);
    // Collect skill data we need into owned types for the closure
    let skills_pipe: Vec<skill_engine::LoadedSkill> = Vec::new(); // Skills are in agent dir, not needed in closure

    let socket = ClientBuilder::new(king_address)
        .namespace("/")
        // Dispatch king:command to role-specific handler
        .on(events::KING_COMMAND, move |payload, _socket| {
            let id = id_cmd.clone();
            let r = role_cmd.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    let stub = soul::Soul {
                        agent_id: id,
                        role: r,
                        behavior: String::new(),
                        body: String::new(),
                    };
                    event_handler::dispatch_command(&stub, events::KING_COMMAND, &data);
                }
            })
        })
        // Dispatch pipeline:next to async role-specific handler
        .on(events::PIPELINE_NEXT, move |payload, socket| {
            let soul = soul_pipe.clone();
            let gateway = Arc::clone(&gateway_pipe);
            let skills = skills_pipe.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    event_handler::dispatch_pipeline_event(
                        &soul, &data, &socket, &gateway, &skills,
                    )
                    .await;
                }
            })
        })
        .on("error", |err, _socket| {
            Box::pin(async move {
                error!(err = ?err, "socket error received");
            })
        })
        .connect()
        .await
        .context("Failed to connect to king Socket.IO server")?;

    // ── Registration ─────────────────────────────────────────────────────────
    info!(agent_id = %agent_id, role = %role, "connected to king, sending registration");
    let reg_payload = json!({
        "agent_id":     agent_id.clone(),
        "role":         role.clone(),
        "capabilities": capabilities,
        "skills":       skill_names,
    });
    if let Err(e) = socket.emit(events::AGENT_REGISTER, reg_payload).await {
        warn!(err = %e, "initial registration emit failed — will retry on next heartbeat");
    }

    // ── Post-connect health check ────────────────────────────────────────────
    info!("running post-connect health check against king");
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    let king_health_url = format!("{}/health", king_address);
    let health_results =
        health_check::check_endpoints(&http_client, &[king_health_url]).await;
    let health_payload = health_check::health_to_json(&agent_id, &health_results);

    let all_healthy = health_results.iter().all(|h| h.reachable);
    if all_healthy {
        info!("king health check passed");
    } else {
        warn!("king health check failed — king may not be fully reachable via HTTP");
    }

    if let Err(e) = socket.emit(events::AGENT_HEALTH, health_payload).await {
        warn!(err = %e, "failed to emit health check results");
    }

    // ── Heartbeat loop ───────────────────────────────────────────────────────
    info!("entering heartbeat loop");

    let mut first = true;
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        // Re-register on first heartbeat as a safety net for reconnects
        if first {
            first = false;
            let reg = json!({
                "agent_id":     agent_id.clone(),
                "role":         role.clone(),
                "capabilities": capabilities,
                "skills":       skill_names,
            });
            if let Err(e) = socket.emit(events::AGENT_REGISTER, reg).await {
                warn!(err = %e, "heartbeat re-registration failed");
            }
        }

        let payload = json!({
            "agent_id": agent_id.clone(),
            "status":   "alive",
        });

        if let Err(e) = socket.emit(events::AGENT_STATUS, payload).await {
            warn!(err = %e, "heartbeat emission failed");
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn payload_to_json(payload: &Payload) -> Option<serde_json::Value> {
    match payload {
        Payload::Text(values) => values.first().cloned(),
        Payload::Binary(data) => serde_json::from_slice(data).ok(),
        _ => None,
    }
}
