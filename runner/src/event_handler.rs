use crate::soul::Soul;
use serde_json::{Value, json};
use tracing::{info, warn};

// ─── Pipeline dispatch ────────────────────────────────────────────────────────

/// Dispatch a `king:command` or `pipeline:next` event to the right handler
/// based on the agent's role.
pub fn dispatch_command(soul: &Soul, event: &str, data: &Value) -> Option<Value> {
    info!(role = %soul.role, event = %event, "dispatching event");

    match soul.role.as_str() {
        "learning" => on_learning(event, data),
        "building" => on_building(event, data),
        "pre-load" => on_pre_load(event, data),
        "evaluation" => on_evaluation(event, data),
        "skill-manage" => on_skill_manage(event, data),
        other => {
            warn!(role = %other, "unknown role — ignoring event");
            None
        }
    }
}

// ─── Role-specific handlers ───────────────────────────────────────────────────

fn on_learning(_event: &str, _data: &Value) -> Option<Value> {
    // TODO: implement skill discovery logic
    // - Monitor configured skill sources
    // - Evaluate discovered skills against system needs
    info!("learning agent: processing discovery task");
    Some(json!({ "status": "discovery_started" }))
}

fn on_building(_event: &str, _data: &Value) -> Option<Value> {
    // TODO: implement skill building / packaging logic
    info!("building agent: processing build task");
    Some(json!({ "status": "build_started" }))
}

fn on_pre_load(_event: &str, data: &Value) -> Option<Value> {
    // TODO: run health checks on skill endpoints before loading
    let skill_id = data["artifact_id"].as_str().unwrap_or("unknown");
    info!(skill_id = %skill_id, "pre-load agent: running health checks");
    Some(json!({ "status": "health_check_started", "skill_id": skill_id }))
}

fn on_evaluation(_event: &str, data: &Value) -> Option<Value> {
    // TODO: evaluate and score a skill
    let skill_id = data["artifact_id"].as_str().unwrap_or("unknown");
    info!(skill_id = %skill_id, "evaluation agent: scoring skill");
    Some(json!({ "status": "evaluation_started", "skill_id": skill_id }))
}

fn on_skill_manage(_event: &str, data: &Value) -> Option<Value> {
    // TODO: activate / deactivate skills based on score threshold
    let skill_id = data["artifact_id"].as_str().unwrap_or("unknown");
    info!(skill_id = %skill_id, "skill-manage agent: managing skill lifecycle");
    Some(json!({ "status": "skill_lifecycle_started", "skill_id": skill_id }))
}
