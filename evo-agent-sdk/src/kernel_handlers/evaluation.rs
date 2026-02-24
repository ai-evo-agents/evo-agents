use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::info;

use crate::handler::{AgentHandler, PipelineContext};

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Default handler for the **Evaluation** kernel agent.
///
/// Scores and benchmarks a skill across multiple dimensions using the LLM.
pub struct EvaluationHandler;

#[async_trait]
impl AgentHandler for EvaluationHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        info!(artifact_id = %ctx.artifact_id, "evaluation agent: scoring skill");

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
            serde_json::to_string_pretty(&ctx.metadata).unwrap_or_default()
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(1024),
            )
            .await?;

        let evaluation = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "raw_response": response }));

        let overall_score = evaluation["overall_score"].as_f64().unwrap_or(0.0);
        let recommendation = evaluation["recommendation"]
            .as_str()
            .unwrap_or("hold")
            .to_string();

        info!(
            artifact_id = %ctx.artifact_id,
            overall_score = %overall_score,
            recommendation = %recommendation,
            "evaluation complete"
        );

        Ok(json!({
            "evaluation": evaluation,
            "artifact_id": ctx.artifact_id,
            "overall_score": overall_score,
            "recommendation": recommendation,
        }))
    }
}
