use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::gateway_client::GatewayClient;
use crate::skill_engine::LoadedSkill;
use crate::soul::Soul;

// ─── Context types ───────────────────────────────────────────────────────────

/// Context provided to [`AgentHandler::on_pipeline`] for every pipeline event.
pub struct PipelineContext<'a> {
    pub soul: &'a Soul,
    pub gateway: &'a Arc<GatewayClient>,
    pub skills: &'a [LoadedSkill],
    pub run_id: String,
    pub stage: String,
    pub artifact_id: String,
    pub metadata: Value,
}

/// Context provided to [`AgentHandler::on_command`] for king commands.
pub struct CommandContext<'a> {
    pub soul: &'a Soul,
    pub event: String,
    pub data: Value,
}

/// Context provided to [`AgentHandler::on_task_evaluate`] for task evaluation events.
pub struct TaskEvaluateContext<'a> {
    pub soul: &'a Soul,
    pub gateway: &'a Arc<GatewayClient>,
    pub task_id: String,
    pub task_type: String,
    pub output_summary: String,
    pub exit_code: Option<i32>,
    pub latency_ms: Option<u64>,
    pub metadata: Value,
}

/// Context provided to [`AgentHandler::on_error_recovery`] for pipeline error analysis.
pub struct ErrorRecoveryContext<'a> {
    pub soul: &'a Soul,
    pub gateway: &'a Arc<GatewayClient>,
    pub request_id: String,
    pub run_id: String,
    pub task_id: String,
    pub failed_stage: String,
    pub error_message: String,
    pub stage_output: Value,
    pub retry_count: u32,
    pub task_summary: String,
}

/// Context provided to [`AgentHandler::on_decompose`] for task decomposition.
pub struct DecomposeContext<'a> {
    pub soul: &'a Soul,
    pub gateway: &'a Arc<GatewayClient>,
    pub request_id: String,
    pub run_id: String,
    pub task_id: String,
    pub task_type: String,
    pub summary: String,
    pub payload: Value,
    pub context: Value,
    pub trigger: String,
}

// ─── AgentHandler trait ──────────────────────────────────────────────────────

/// Trait for handling agent events.
///
/// Implement this trait to create custom agent behavior. The SDK provides
/// default kernel handler implementations in [`crate::kernel_handlers`].
///
/// # Example
///
/// ```rust,ignore
/// use async_trait::async_trait;
/// use evo_agent_sdk::{AgentHandler, PipelineContext};
///
/// struct MyAgent;
///
/// #[async_trait]
/// impl AgentHandler for MyAgent {
///     async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<serde_json::Value> {
///         let response = ctx.gateway
///             .chat_completion("gpt-4o-mini", &ctx.soul.behavior, "Hello", None, None)
///             .await?;
///         Ok(serde_json::json!({ "result": response }))
///     }
/// }
/// ```
#[async_trait]
pub trait AgentHandler: Send + Sync + 'static {
    /// Handle a `pipeline:next` event. Return output JSON on success.
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value>;

    /// Handle a `king:command` event. Default implementation logs and ignores.
    fn on_command(&self, ctx: &CommandContext<'_>) {
        tracing::info!(
            role = %ctx.soul.role,
            event = %ctx.event,
            command = %ctx.data["command"].as_str().unwrap_or("unknown"),
            "king command received"
        );
    }

    /// Handle a `task:evaluate` event. Override to produce task summaries.
    /// Default implementation is a no-op (returns `Value::Null`).
    async fn on_task_evaluate(&self, _ctx: TaskEvaluateContext<'_>) -> anyhow::Result<Value> {
        Ok(Value::Null)
    }

    /// Handle an `error:recovery_request` event. Override to provide error analysis.
    /// Default implementation recommends abort.
    async fn on_error_recovery(&self, _ctx: ErrorRecoveryContext<'_>) -> anyhow::Result<Value> {
        Ok(
            serde_json::json!({ "action": "abort", "reasoning": "no error recovery handler", "params": {} }),
        )
    }

    /// Handle a `task:decompose` event. Override to break tasks into subtasks.
    /// Default implementation returns empty subtasks (no decomposition).
    async fn on_decompose(&self, _ctx: DecomposeContext<'_>) -> anyhow::Result<Value> {
        Ok(
            serde_json::json!({ "should_decompose": false, "subtasks": [], "reasoning": "no decomposition handler" }),
        )
    }
}
