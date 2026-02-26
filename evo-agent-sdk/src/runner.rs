use anyhow::{Context, Result, bail};
use evo_common::{logging::init_logging, messages::events};
use rust_socketio::{Payload, asynchronous::ClientBuilder};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};
use tracing::{error, info, warn};

use crate::gateway_client::GatewayClient;
use crate::handler::{AgentHandler, CommandContext, PipelineContext, TaskEvaluateContext};
use crate::health_check;
use crate::kernel_handlers::*;
use crate::skill_engine::{self, LoadedSkill};
use crate::soul::{self, Soul};

// ─── AgentRunner ─────────────────────────────────────────────────────────────

/// Boots an agent: loads soul, connects to king, dispatches events, runs heartbeat.
///
/// # Usage
///
/// With a custom handler:
/// ```rust,ignore
/// AgentRunner::run(MyHandler).await?;
/// ```
///
/// With auto-dispatch for kernel roles:
/// ```rust,ignore
/// AgentRunner::run_kernel().await?;
/// ```
pub struct AgentRunner;

impl AgentRunner {
    /// Run an agent with the given handler.
    ///
    /// Parses CLI args (or `AGENT_FOLDER` env) for the agent directory,
    /// loads `soul.md` and skills, connects to king, and enters the event loop.
    pub async fn run<H: AgentHandler>(handler: H) -> Result<()> {
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
        let gateway_address = std::env::var("GATEWAY_ADDRESS")
            .unwrap_or_else(|_| "http://localhost:8080".to_string());

        info!(king = %king_address, gateway = %gateway_address, "connecting to king");

        // Create gateway client for LLM calls
        let gateway = Arc::new(
            GatewayClient::new(&gateway_address).context("Failed to create gateway client")?,
        );

        run_client(&soul, &king_address, &skills, &gateway, handler).await?;

        Ok(())
    }

    /// Convenience: auto-dispatch to the correct kernel handler based on `soul.md` role.
    ///
    /// Reads the agent directory, parses the role from `soul.md`, and runs the
    /// matching kernel handler. Returns an error for unknown roles.
    pub async fn run_kernel() -> Result<()> {
        // We need to peek at the soul to determine the role before dispatching
        let agent_folder = std::env::args()
            .nth(1)
            .unwrap_or_else(|| std::env::var("AGENT_FOLDER").unwrap_or_else(|_| ".".to_string()));

        let agent_dir = PathBuf::from(&agent_folder);
        if !agent_dir.exists() {
            bail!("Agent folder does not exist: {}", agent_dir.display());
        }

        let soul = soul::load_soul(&agent_dir)
            .with_context(|| format!("Failed to load soul from {}", agent_dir.display()))?;

        match soul.role.as_str() {
            "learning" => Self::run(LearningHandler).await,
            "building" => Self::run(BuildingHandler).await,
            "pre-load" | "pre_load" => Self::run(PreLoadHandler).await,
            "evaluation" => Self::run(EvaluationHandler).await,
            "skill-manage" | "skill_manage" => Self::run(SkillManageHandler).await,
            other => bail!(
                "Unknown kernel role: {other}. Use AgentRunner::run(handler) for custom agents."
            ),
        }
    }
}

// ─── Socket.IO client loop ────────────────────────────────────────────────────

async fn run_client<H: AgentHandler>(
    soul: &Soul,
    king_address: &str,
    skills: &[LoadedSkill],
    gateway: &Arc<GatewayClient>,
    handler: H,
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

    // Wrap handler in Arc for shared ownership across closures
    let handler = Arc::new(handler);

    // Clone identifiers for each closure
    let (id_cmd, role_cmd) = (agent_id.clone(), role.clone());

    // Clones for command handler
    let handler_cmd = Arc::clone(&handler);

    // Clones for pipeline handler
    let soul_pipe = soul.clone();
    let gateway_pipe = Arc::clone(gateway);
    let handler_pipe = Arc::clone(&handler);

    // Clones for debug prompt handler
    let soul_debug = soul.clone();
    let gateway_debug = Arc::clone(gateway);
    let id_debug = agent_id.clone();
    let role_debug = role.clone();

    // Clones for task:invite handler
    let id_invite = agent_id.clone();

    // Clones for task:evaluate handler
    let soul_eval = soul.clone();
    let gateway_eval = Arc::clone(gateway);
    let handler_eval = Arc::clone(&handler);
    let id_eval = agent_id.clone();

    let socket = ClientBuilder::new(king_address)
        .namespace("/")
        // Dispatch king:command via handler
        .on(events::KING_COMMAND, move |payload, _socket| {
            let id = id_cmd.clone();
            let r = role_cmd.clone();
            let h = Arc::clone(&handler_cmd);
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    let stub = Soul {
                        agent_id: id,
                        role: r,
                        behavior: String::new(),
                        body: String::new(),
                    };
                    let ctx = CommandContext {
                        soul: &stub,
                        event: events::KING_COMMAND.to_string(),
                        data,
                    };
                    h.on_command(&ctx);
                }
            })
        })
        // Dispatch pipeline:next via handler
        .on(events::PIPELINE_NEXT, move |payload, socket| {
            let soul = soul_pipe.clone();
            let gateway = Arc::clone(&gateway_pipe);
            let h = Arc::clone(&handler_pipe);
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    dispatch_pipeline(&soul, &data, &socket, &gateway, &[], &*h).await;
                }
            })
        })
        // Dispatch debug:prompt — send prompt to gateway, return response
        .on(events::DEBUG_PROMPT, move |payload, socket| {
            let soul = soul_debug.clone();
            let gateway = Arc::clone(&gateway_debug);
            let id = id_debug.clone();
            let r = role_debug.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    dispatch_debug_prompt(&soul, &data, &socket, &gateway, &id, &r).await;
                }
            })
        })
        .on(events::TASK_INVITE, move |payload, socket| {
            let id = id_invite.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    let task_id = data["task_id"].as_str().unwrap_or("");
                    if !task_id.is_empty() {
                        let join_payload = json!({ "task_id": task_id, "agent_id": id });
                        if let Err(e) = socket.emit(events::TASK_JOIN, join_payload).await {
                            warn!(err = %e, "failed to emit task:join");
                        } else {
                            info!(task_id = %task_id, "joined task room");
                        }
                    }
                }
            })
        })
        .on(events::TASK_EVALUATE, move |payload, socket| {
            let soul = soul_eval.clone();
            let gateway = Arc::clone(&gateway_eval);
            let h = Arc::clone(&handler_eval);
            let agent_id = id_eval.clone();
            Box::pin(async move {
                if let Some(data) = payload_to_json(&payload) {
                    dispatch_task_evaluate(&soul, &data, &socket, &gateway, &agent_id, &*h).await;
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
    let health_results = health_check::check_endpoints(&http_client, &[king_health_url]).await;
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

// ─── Pipeline dispatch ────────────────────────────────────────────────────────

async fn dispatch_pipeline(
    soul: &Soul,
    data: &Value,
    socket: &rust_socketio::asynchronous::Client,
    gateway: &Arc<GatewayClient>,
    skills: &[LoadedSkill],
    handler: &dyn AgentHandler,
) {
    let run_id = data["run_id"].as_str().unwrap_or("unknown").to_string();
    let stage = data["stage"].as_str().unwrap_or("unknown").to_string();
    let artifact_id = data["artifact_id"].as_str().unwrap_or("").to_string();
    let metadata = data.get("metadata").cloned().unwrap_or(Value::Null);

    info!(
        role = %soul.role,
        run_id = %run_id,
        stage = %stage,
        "processing pipeline event"
    );

    let ctx = PipelineContext {
        soul,
        gateway,
        skills,
        run_id: run_id.clone(),
        stage: stage.clone(),
        artifact_id: artifact_id.clone(),
        metadata,
    };

    let result = handler.on_pipeline(ctx).await;

    // Emit pipeline:stage_result back to king
    let (status, output, error_msg) = match result {
        Ok(output) => ("completed", output, None),
        Err(e) => {
            error!(
                role = %soul.role,
                run_id = %run_id,
                err = %e,
                "pipeline stage failed"
            );
            ("failed", Value::Null, Some(e.to_string()))
        }
    };

    let stage_result = json!({
        "run_id": run_id,
        "stage": stage,
        "agent_id": soul.agent_id,
        "status": status,
        "artifact_id": artifact_id,
        "output": output,
        "error": error_msg,
    });

    if let Err(e) = socket
        .emit(events::PIPELINE_STAGE_RESULT, stage_result)
        .await
    {
        error!(
            run_id = %run_id,
            stage = %stage,
            err = %e,
            "failed to emit pipeline:stage_result"
        );
    }
}

// ─── Task evaluate dispatch ──────────────────────────────────────────────────

async fn dispatch_task_evaluate(
    soul: &Soul,
    data: &Value,
    socket: &rust_socketio::asynchronous::Client,
    gateway: &Arc<GatewayClient>,
    agent_id: &str,
    handler: &dyn AgentHandler,
) {
    let task_id = data["task_id"].as_str().unwrap_or("unknown").to_string();
    let task_type = data["task_type"].as_str().unwrap_or("unknown").to_string();
    let output_summary = data["output_summary"].as_str().unwrap_or("").to_string();
    let exit_code = data["exit_code"].as_i64().map(|n| n as i32);
    let latency_ms = data["latency_ms"].as_u64();
    let metadata = data.get("metadata").cloned().unwrap_or(Value::Null);

    info!(task_id = %task_id, task_type = %task_type, role = %soul.role, "processing task:evaluate");

    let ctx = TaskEvaluateContext {
        soul,
        gateway,
        task_id: task_id.clone(),
        task_type,
        output_summary,
        exit_code,
        latency_ms,
        metadata,
    };

    match handler.on_task_evaluate(ctx).await {
        Ok(Value::Null) => {} // no-op
        Ok(output) => {
            let summary_payload = json!({
                "task_id": task_id,
                "agent_id": agent_id,
                "summary": output["summary"].as_str().unwrap_or(""),
                "score": output["score"].as_f64(),
                "tags": output.get("tags").cloned().unwrap_or(json!([])),
                "evaluation": output,
            });
            if let Err(e) = socket.emit(events::TASK_SUMMARY, summary_payload).await {
                error!(task_id = %task_id, err = %e, "failed to emit task:summary");
            }
        }
        Err(e) => warn!(task_id = %task_id, err = %e, "task evaluation failed"),
    }
}

// ─── Debug prompt dispatch ────────────────────────────────────────────────────

async fn dispatch_debug_prompt(
    soul: &Soul,
    data: &Value,
    socket: &rust_socketio::asynchronous::Client,
    gateway: &Arc<GatewayClient>,
    agent_id: &str,
    role: &str,
) {
    let request_id = data["request_id"].as_str().unwrap_or("unknown").to_string();
    let task_id = data["task_id"].as_str().map(|s| s.to_string());
    let model = data["model"].as_str().unwrap_or("gpt-4o-mini").to_string();
    let prompt = data["prompt"].as_str().unwrap_or("").to_string();
    let temperature = data["temperature"].as_f64();
    let max_tokens = data["max_tokens"].as_u64().map(|n| n as u32);

    // Prepend provider prefix if specified
    let full_model = match data["provider"].as_str() {
        Some(p) if !p.is_empty() => format!("{p}:{model}"),
        _ => model.clone(),
    };

    info!(
        agent_id = %agent_id,
        request_id = %request_id,
        model = %full_model,
        "processing debug prompt (streaming)"
    );

    let start = std::time::Instant::now();

    // Channel to bridge sync on_chunk callback to async Socket.IO emit
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, u32)>();

    // Spawn a task to forward stream chunks via Socket.IO
    let socket_clone = socket.clone();
    let req_id_clone = request_id.clone();
    let task_id_clone = task_id.clone();
    let emit_task = tokio::spawn(async move {
        while let Some((delta, chunk_index)) = rx.recv().await {
            let mut chunk_payload = json!({
                "request_id": req_id_clone,
                "delta": delta,
                "chunk_index": chunk_index,
            });
            if let Some(ref tid) = task_id_clone {
                chunk_payload["task_id"] = json!(tid);
            }
            if let Err(e) = socket_clone.emit(events::DEBUG_STREAM, chunk_payload).await {
                warn!(err = %e, "failed to emit debug:stream chunk");
            }
        }
    });

    let result = gateway
        .chat_completion_streaming(
            &full_model,
            &soul.behavior,
            &prompt,
            temperature,
            max_tokens,
            |delta: &str, chunk_index: u32| {
                let _ = tx.send((delta.to_string(), chunk_index));
            },
        )
        .await;

    // Drop sender so the emit task drains remaining chunks and exits
    drop(tx);
    let _ = emit_task.await;

    let latency_ms = start.elapsed().as_millis() as u64;

    let response = match result {
        Ok(text) => {
            let mut payload = json!({
                "request_id": request_id,
                "agent_id": agent_id,
                "role": role,
                "model": full_model,
                "response": text,
                "latency_ms": latency_ms,
            });
            if let Some(ref tid) = task_id {
                payload["task_id"] = json!(tid);
            }
            payload
        }
        Err(e) => {
            error!(
                request_id = %request_id,
                err = %e,
                "debug prompt streaming failed"
            );
            let mut payload = json!({
                "request_id": request_id,
                "agent_id": agent_id,
                "role": role,
                "model": full_model,
                "error": e.to_string(),
                "latency_ms": latency_ms,
            });
            if let Some(ref tid) = task_id {
                payload["task_id"] = json!(tid);
            }
            payload
        }
    };

    if let Err(e) = socket.emit(events::DEBUG_RESPONSE, response).await {
        error!(
            request_id = %request_id,
            err = %e,
            "failed to emit debug:response"
        );
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn payload_to_json(payload: &Payload) -> Option<Value> {
    match payload {
        Payload::Text(values) => values.first().cloned(),
        Payload::Binary(data) => serde_json::from_slice(data).ok(),
        _ => None,
    }
}
