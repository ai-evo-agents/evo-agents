use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::info;

use crate::handler::{AgentHandler, PipelineContext};
use crate::self_upgrade;

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Default handler for the **Evaluation** kernel agent.
///
/// Two modes:
/// - **Skill evaluation** (default): Scores and benchmarks a skill across
///   multiple dimensions using the LLM.
/// - **Self-upgrade evaluation** (`build_type: "self_upgrade"`): Compares
///   new version vs current, verifies all pre-load checks passed, and
///   produces a pass/fail verdict.
pub struct EvaluationHandler;

#[async_trait]
impl AgentHandler for EvaluationHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        if self_upgrade::is_self_upgrade(&ctx.metadata) {
            return self.evaluate_upgrade(&ctx).await;
        }

        self.evaluate_skill(&ctx).await
    }
}

impl EvaluationHandler {
    /// Original LLM-based skill evaluation.
    async fn evaluate_skill(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
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

    /// Self-upgrade: evaluate the new release against current version.
    async fn evaluate_upgrade(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        let component = ctx.metadata["component"]
            .as_str()
            .unwrap_or(&ctx.artifact_id);
        let new_version = ctx.metadata["new_version"]
            .as_str()
            .unwrap_or("v0.0.0");

        info!(
            component,
            new_version,
            run_id = %ctx.run_id,
            "evaluation agent: evaluating self-upgrade"
        );

        // Check that pre-load validation passed
        let preload_passed = ctx.metadata["validation"]["all_passed"]
            .as_bool()
            .unwrap_or(false);

        if !preload_passed {
            return Ok(json!({
                "build_type": "self_upgrade",
                "component": component,
                "new_version": new_version,
                "overall_score": 0.0,
                "recommendation": "discard",
                "reasoning": "Pre-load validation did not pass. Cannot approve upgrade.",
                "artifact_id": ctx.artifact_id,
            }));
        }

        let eval_result = self_upgrade::evaluate_upgrade(component, new_version).await?;

        let overall_score = eval_result["overall_score"].as_f64().unwrap_or(0.0);
        let recommendation = eval_result["recommendation"]
            .as_str()
            .unwrap_or("hold")
            .to_string();

        info!(
            component,
            new_version,
            overall_score = %overall_score,
            recommendation = %recommendation,
            "self-upgrade evaluation complete"
        );

        Ok(json!({
            "build_type": "self_upgrade",
            "evaluation": eval_result,
            "artifact_id": ctx.artifact_id,
            "overall_score": overall_score,
            "recommendation": recommendation,
        }))
    }
}
