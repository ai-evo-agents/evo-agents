use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::info;

use crate::handler::{AgentHandler, PipelineContext};

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Default handler for the **Learning** kernel agent.
///
/// Discovers potential new skills by querying the LLM via the gateway.
pub struct LearningHandler;

#[async_trait]
impl AgentHandler for LearningHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        info!("learning agent: starting skill discovery");

        let existing_skills: Vec<&str> = ctx.skills.iter().map(|s| s.name.as_str()).collect();

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
            serde_json::to_string_pretty(&ctx.metadata).unwrap_or_default()
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.7),
                Some(1024),
            )
            .await?;

        // Try to parse as JSON, fall back to wrapping in object
        let candidates = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "raw_response": response }));

        info!(
            candidates = %candidates,
            "learning agent: discovery complete"
        );

        Ok(json!({
            "candidates": candidates,
            "existing_skills": existing_skills,
        }))
    }
}
