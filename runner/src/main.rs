mod event_handler;
mod health_check;
mod skill_engine;
mod soul;

use anyhow::{bail, Context, Result};
use evo_common::{logging::init_logging, messages::events};
use rust_socketio::{
    asynchronous::ClientBuilder,
    Payload,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
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
        "runner starting"
    );

    // Load available skills
    let skills = skill_engine::load_skills(&agent_dir);
    info!(skills = skills.len(), "skills loaded");

    // King address (Socket.IO server)
    let king_address = std::env::var("KING_ADDRESS")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    info!(king = %king_address, "connecting to king");

    run_client(&soul, &king_address).await?;

    Ok(())
}

// ─── Socket.IO client loop ────────────────────────────────────────────────────

async fn run_client(soul: &soul::Soul, king_address: &str) -> Result<()> {
    let agent_id = soul.agent_id.clone();
    let role = soul.role.clone();

    // Clone identifiers for each closure
    let (id_reg, role_reg) = (agent_id.clone(), role.clone());
    let (id_cmd, role_cmd) = (agent_id.clone(), role.clone());
    let (id_pipe, role_pipe) = (agent_id.clone(), role.clone());
    let id_hb = agent_id.clone();

    let socket = ClientBuilder::new(king_address)
        .namespace("/")
        // Register when connection is established
        .on("connect", move |_payload, socket| {
            let agent_id = id_reg.clone();
            let role = role_reg.clone();
            Box::pin(async move {
                info!(agent_id = %agent_id, "connected to king, sending registration");
                let payload = json!({
                    "agent_id":     agent_id,
                    "role":         role,
                    "capabilities": [],
                });
                if let Err(e) = socket.emit(events::AGENT_REGISTER, payload).await {
                    error!(err = %e, "failed to emit agent:register");
                }
            })
        })
        // Dispatch king:command to role-specific handler
        .on(events::KING_COMMAND, move |payload, _socket| {
            let id = id_cmd.clone();
            let r = role_cmd.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    let stub = soul::Soul { agent_id: id, role: r, body: String::new() };
                    event_handler::dispatch_command(&stub, events::KING_COMMAND, &data);
                }
            })
        })
        // Dispatch pipeline:next to role-specific handler
        .on(events::PIPELINE_NEXT, move |payload, _socket| {
            let id = id_pipe.clone();
            let r = role_pipe.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    let stub = soul::Soul { agent_id: id, role: r, body: String::new() };
                    event_handler::dispatch_command(&stub, events::PIPELINE_NEXT, &data);
                }
            })
        })
        .on("error", |err, _socket| Box::pin(async move {
            error!(err = ?err, "socket error received");
        }))
        .connect()
        .await
        .context("Failed to connect to king Socket.IO server")?;

    info!("socket connected — entering heartbeat loop");

    // Heartbeat every 30 seconds
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        let payload = json!({
            "agent_id": id_hb,
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
