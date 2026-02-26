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
}
