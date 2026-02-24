use crate::gateway_client::GatewayClient;
use crate::health_check;
use crate::skill_engine::LoadedSkill;
use crate::soul::Soul;
use evo_common::messages::events;
use rust_socketio::asynchronous::Client;
use serde_json::{Value, json};
use tracing::{error, info, warn};

/// Default LLM model to use via gateway.
const DEFAULT_MODEL: &str = "gpt-4o-mini";

// ─── Pipeline dispatch ────────────────────────────────────────────────────────

/// Dispatch a `pipeline:next` event to the correct async handler
/// based on the agent's role.
///
/// On completion (success or failure), emits `pipeline:stage_result` back to king.
pub async fn dispatch_pipeline_event(
    soul: &Soul,
    data: &Value,
    socket: &Client,
    gateway: &GatewayClient,
    skills: &[LoadedSkill],
) {
    let run_id = data["run_id"].as_str().unwrap_or("unknown");
    let stage = data["stage"].as_str().unwrap_or("unknown");
    let artifact_id = data["artifact_id"].as_str().unwrap_or("");
    let metadata = data.get("metadata").cloned().unwrap_or(Value::Null);

    info!(
        role = %soul.role,
        run_id = %run_id,
        stage = %stage,
        "processing pipeline event"
    );

    let result = match soul.role.as_str() {
        "learning" => on_learning(soul, &metadata, gateway, skills).await,
        "building" => on_building(soul, artifact_id, &metadata, gateway).await,
        "pre-load" | "pre_load" => on_pre_load(artifact_id, &metadata).await,
        "evaluation" => on_evaluation(soul, artifact_id, &metadata, gateway).await,
        "skill-manage" | "skill_manage" => on_skill_manage(soul, artifact_id, &metadata, gateway).await,
        other => {
            warn!(role = %other, "unknown role — cannot handle pipeline event");
            Err(anyhow::anyhow!("unknown role: {other}"))
        }
    };

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

/// Dispatch a `king:command` event (non-pipeline, synchronous logging only).
pub fn dispatch_command(soul: &Soul, event: &str, data: &Value) {
    info!(
        role = %soul.role,
        event = %event,
        command = %data["command"].as_str().unwrap_or("unknown"),
        "king command received"
    );
}

// ─── Role-specific handlers ───────────────────────────────────────────────────

/// Learning agent: discover potential new skills.
///
/// Uses LLM to evaluate skill sources and identify candidates.
async fn on_learning(
    soul: &Soul,
    metadata: &Value,
    gateway: &GatewayClient,
    skills: &[LoadedSkill],
) -> anyhow::Result<Value> {
    info!("learning agent: starting skill discovery");

    let existing_skills: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();

    let prompt = format!(
        "You are a skill discovery agent for an AI self-evolution system.\n\
         Existing skills: {:?}\n\
         Trigger metadata: {}\n\n\
         Identify 1-3 potential new skills that would complement the existing set.\n\
         For each candidate, provide:\n\
         - name: a short kebab-case identifier\n\
         - description: what the skill does\n\
         - source: where it could be obtained (API, registry, etc.)\n\
         - priority: high/medium/low\n\n\
         Respond with valid JSON array of candidates.",
        existing_skills,
        serde_json::to_string_pretty(metadata).unwrap_or_default()
    );

    let response = gateway
        .chat_completion(DEFAULT_MODEL, &soul.behavior, &prompt, Some(0.7), Some(1024))
        .await?;

    // Try to parse as JSON, fall back to wrapping in object
    let candidates = serde_json::from_str::<Value>(&response).unwrap_or_else(|_| {
        json!({ "raw_response": response })
    });

    info!(
        candidates = %candidates,
        "learning agent: discovery complete"
    );

    Ok(json!({
        "candidates": candidates,
        "existing_skills": existing_skills,
    }))
}

/// Building agent: package a discovered skill into manifest + config.
///
/// Uses LLM to generate `manifest.toml` and `config.toml` from candidate data.
async fn on_building(
    soul: &Soul,
    artifact_id: &str,
    metadata: &Value,
    gateway: &GatewayClient,
) -> anyhow::Result<Value> {
    info!(artifact_id = %artifact_id, "building agent: packaging skill");

    let prompt = format!(
        "You are a skill builder for an AI self-evolution system.\n\
         Build a skill package for the following candidate:\n\
         {}\n\n\
         Generate:\n\
         1. A manifest.toml with: name, version (0.1.0), description, capabilities (array), \
            has_code (false for API-only), dependencies (array), inputs (array of name/type/required/description), \
            outputs (array of name/type/required/description)\n\
         2. A config.toml with: auth_ref (env var name), endpoints (array of name/url/method)\n\n\
         Respond with JSON object containing 'manifest_toml' and 'config_toml' as strings.",
        serde_json::to_string_pretty(metadata).unwrap_or_default()
    );

    let response = gateway
        .chat_completion(DEFAULT_MODEL, &soul.behavior, &prompt, Some(0.3), Some(2048))
        .await?;

    let build_output = serde_json::from_str::<Value>(&response).unwrap_or_else(|_| {
        json!({ "raw_response": response })
    });

    // Validate manifest if present
    if let Some(manifest_str) = build_output["manifest_toml"].as_str() {
        match toml::from_str::<evo_common::skill::SkillManifest>(manifest_str) {
            Ok(manifest) => {
                info!(
                    skill = %manifest.name,
                    capabilities = ?manifest.capabilities,
                    "manifest validated successfully"
                );
            }
            Err(e) => {
                warn!(err = %e, "generated manifest failed validation");
            }
        }
    }

    Ok(json!({
        "build_output": build_output,
        "artifact_id": artifact_id,
    }))
}

/// Pre-load agent: health-check skill API endpoints.
///
/// Does NOT use LLM — purely endpoint validation.
async fn on_pre_load(
    artifact_id: &str,
    metadata: &Value,
) -> anyhow::Result<Value> {
    info!(artifact_id = %artifact_id, "pre-load agent: health-checking endpoints");

    // Extract endpoint URLs from build output config
    let mut urls_to_check = Vec::new();

    if let Some(config_str) = metadata["build_output"]["config_toml"].as_str() {
        if let Ok(config) = toml::from_str::<evo_common::skill::SkillConfig>(config_str) {
            for endpoint in &config.endpoints {
                urls_to_check.push(endpoint.url.clone());
            }
        }
    }

    // Also check any URLs in the metadata directly
    if let Some(endpoints) = metadata["endpoints"].as_array() {
        for ep in endpoints {
            if let Some(url) = ep["url"].as_str() {
                urls_to_check.push(url.to_string());
            }
        }
    }

    if urls_to_check.is_empty() {
        info!("no endpoints to check — passing pre-load");
        return Ok(json!({
            "health_results": [],
            "all_healthy": true,
            "message": "no endpoints to validate"
        }));
    }

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let results = health_check::check_endpoints(&http_client, &urls_to_check).await;

    let all_healthy = results.iter().all(|h| h.reachable);
    let health_json: Vec<Value> = results
        .iter()
        .map(|h| {
            json!({
                "url": h.url,
                "reachable": h.reachable,
                "latency_ms": h.latency_ms,
                "status_code": h.status_code,
            })
        })
        .collect();

    if !all_healthy {
        let failed: Vec<&str> = results
            .iter()
            .filter(|h| !h.reachable)
            .map(|h| h.url.as_str())
            .collect();
        warn!(failed = ?failed, "some endpoints failed health check");
        return Err(anyhow::anyhow!(
            "health check failed for endpoints: {:?}",
            failed
        ));
    }

    info!(
        checked = results.len(),
        "all endpoints healthy"
    );

    Ok(json!({
        "health_results": health_json,
        "all_healthy": all_healthy,
    }))
}

/// Evaluation agent: score and benchmark a skill.
///
/// Uses LLM to score across multiple dimensions.
async fn on_evaluation(
    soul: &Soul,
    artifact_id: &str,
    metadata: &Value,
    gateway: &GatewayClient,
) -> anyhow::Result<Value> {
    info!(artifact_id = %artifact_id, "evaluation agent: scoring skill");

    let prompt = format!(
        "You are a skill evaluator for an AI self-evolution system.\n\
         Evaluate the following skill:\n\
         {}\n\n\
         Score it on these dimensions (0.0 to 1.0):\n\
         1. utility: How useful is this skill to the system?\n\
         2. reliability: How reliable are the endpoints/APIs?\n\
         3. novelty: Does it add genuinely new capabilities?\n\
         4. integration: How well does it fit with existing skills?\n\n\
         Also provide:\n\
         - overall_score: weighted average (utility=0.4, reliability=0.3, novelty=0.2, integration=0.1)\n\
         - recommendation: 'activate', 'hold', or 'discard'\n\
         - reasoning: brief explanation\n\n\
         Respond with valid JSON.",
        serde_json::to_string_pretty(metadata).unwrap_or_default()
    );

    let response = gateway
        .chat_completion(DEFAULT_MODEL, &soul.behavior, &prompt, Some(0.3), Some(1024))
        .await?;

    let evaluation = serde_json::from_str::<Value>(&response).unwrap_or_else(|_| {
        json!({ "raw_response": response })
    });

    let overall_score = evaluation["overall_score"].as_f64().unwrap_or(0.0);
    let recommendation = evaluation["recommendation"]
        .as_str()
        .unwrap_or("hold")
        .to_string();

    info!(
        artifact_id = %artifact_id,
        overall_score = %overall_score,
        recommendation = %recommendation,
        "evaluation complete"
    );

    Ok(json!({
        "evaluation": evaluation,
        "artifact_id": artifact_id,
        "overall_score": overall_score,
        "recommendation": recommendation,
    }))
}

/// Skill manage agent: activate, hold, or discard based on evaluation.
///
/// Uses LLM to determine target agents for activation and plan deployment.
async fn on_skill_manage(
    soul: &Soul,
    artifact_id: &str,
    metadata: &Value,
    gateway: &GatewayClient,
) -> anyhow::Result<Value> {
    let recommendation = metadata["recommendation"]
        .as_str()
        .unwrap_or("hold");
    let overall_score = metadata["overall_score"].as_f64().unwrap_or(0.0);

    info!(
        artifact_id = %artifact_id,
        recommendation = %recommendation,
        score = %overall_score,
        "skill-manage agent: processing lifecycle decision"
    );

    // Activation threshold
    const ACTIVATION_THRESHOLD: f64 = 0.6;

    if recommendation == "discard" || overall_score < ACTIVATION_THRESHOLD {
        info!(
            artifact_id = %artifact_id,
            "skill discarded (below threshold or recommendation=discard)"
        );
        return Ok(json!({
            "action": "discarded",
            "artifact_id": artifact_id,
            "reason": format!(
                "score {overall_score:.2} below threshold {ACTIVATION_THRESHOLD} or recommendation=discard"
            ),
        }));
    }

    // Use LLM to plan deployment
    let prompt = format!(
        "You are a skill deployment manager for an AI self-evolution system.\n\
         A skill has passed evaluation and should be activated.\n\
         Skill data: {}\n\n\
         Determine:\n\
         1. target_agents: Which user agents should receive this skill? (array of role names)\n\
         2. deployment_notes: Any special configuration needed\n\
         3. rollback_plan: How to revert if the skill causes issues\n\n\
         Respond with valid JSON.",
        serde_json::to_string_pretty(metadata).unwrap_or_default()
    );

    let response = gateway
        .chat_completion(DEFAULT_MODEL, &soul.behavior, &prompt, Some(0.3), Some(1024))
        .await?;

    let deployment = serde_json::from_str::<Value>(&response).unwrap_or_else(|_| {
        json!({ "raw_response": response })
    });

    info!(
        artifact_id = %artifact_id,
        action = "activated",
        "skill lifecycle complete"
    );

    Ok(json!({
        "action": "activated",
        "artifact_id": artifact_id,
        "deployment": deployment,
        "overall_score": overall_score,
    }))
}
