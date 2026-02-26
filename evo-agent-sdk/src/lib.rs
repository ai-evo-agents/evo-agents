//! # evo-agent-sdk
//!
//! SDK for building agents in the Evo self-evolution system.
//!
//! This crate provides the core infrastructure for creating agents that connect
//! to the Evo king orchestrator via Socket.IO, handle pipeline events, and
//! interact with LLMs through the evo-gateway.
//!
//! # Quick Start
//!
//! ## Kernel agent (using built-in handler)
//!
//! ```rust,ignore
//! use evo_agent_sdk::{AgentRunner, kernel_handlers::LearningHandler};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     AgentRunner::run(LearningHandler).await
//! }
//! ```
//!
//! ## Custom agent
//!
//! ```rust,ignore
//! use async_trait::async_trait;
//! use evo_agent_sdk::prelude::*;
//!
//! struct MyAgent;
//!
//! #[async_trait]
//! impl AgentHandler for MyAgent {
//!     async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<serde_json::Value> {
//!         let response = ctx.gateway
//!             .chat_completion("gpt-4o-mini", &ctx.soul.behavior, "Hello", None, None)
//!             .await?;
//!         Ok(serde_json::json!({ "result": response }))
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     AgentRunner::run(MyAgent).await
//! }
//! ```

pub mod gateway_client;
pub mod handler;
pub mod health_check;
pub mod kernel_handlers;
pub mod runner;
pub mod self_upgrade;
pub mod skill_engine;
pub mod soul;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use gateway_client::GatewayClient;
pub use handler::{AgentHandler, CommandContext, PipelineContext, TaskEvaluateContext};
pub use runner::AgentRunner;
pub use skill_engine::LoadedSkill;
pub use soul::Soul;

/// Convenience re-export of `evo_common` for downstream crates.
pub use evo_common;

// ─── Prelude ─────────────────────────────────────────────────────────────────

/// Import everything you need for a custom agent in one line:
///
/// ```rust,ignore
/// use evo_agent_sdk::prelude::*;
/// ```
pub mod prelude {
    pub use crate::gateway_client::GatewayClient;
    pub use crate::handler::{AgentHandler, CommandContext, PipelineContext, TaskEvaluateContext};
    pub use crate::runner::AgentRunner;
    pub use crate::skill_engine::LoadedSkill;
    pub use crate::soul::Soul;
    pub use serde_json::{self, json};
}
