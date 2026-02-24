use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::info;

use crate::handler::{AgentHandler, PipelineContext};
use crate::self_upgrade;

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Activation score threshold. Skills below this are discarded.
const ACTIVATION_THRESHOLD: f64 = 0.6;

/// Default handler for the **Skill Manage** kernel agent.
///
/// Two modes:
/// - **Skill management** (default): Decides whether to activate, hold, or
///   discard a skill based on evaluation scores and LLM-guided deployment.
/// - **Self-upgrade management** (`build_type: "self_upgrade"`): Approves
///   or rejects the upgrade, passing through component info for king to
///   trigger `update.sh`.
pub struct SkillManageHandler;

#[async_trait]
impl AgentHandler for SkillManageHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        if self_upgrade::is_self_upgrade(&ctx.metadata) {
            return self.manage_upgrade(&ctx).await;
        }

        self.manage_skill(&ctx).await
    }
}

impl SkillManageHandler {
    /// Original skill lifecycle management.
    async fn manage_skill(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        let recommendation = ctx.metadata["recommendation"].as_str().unwrap_or("hold");
        let overall_score = ctx.metadata["overall_score"].as_f64().unwrap_or(0.0);

        info!(
            artifact_id = %ctx.artifact_id,
            recommendation = %recommendation,
            score = %overall_score,
            "skill-manage agent: processing lifecycle decision"
        );

        if recommendation == "discard" || overall_score < ACTIVATION_THRESHOLD {
            info!(
                artifact_id = %ctx.artifact_id,
                "skill discarded (below threshold or recommendation=discard)"
            );
            return Ok(json!({
                "action": "discarded",
                "artifact_id": ctx.artifact_id,
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

        let deployment = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "raw_response": response }));

        info!(
            artifact_id = %ctx.artifact_id,
            action = "activated",
            "skill lifecycle complete"
        );

        Ok(json!({
            "action": "activated",
            "artifact_id": ctx.artifact_id,
            "deployment": deployment,
            "overall_score": overall_score,
        }))
    }

    /// Self-upgrade: approve or reject the upgrade based on evaluation.
    async fn manage_upgrade(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        let component = ctx.metadata["evaluation"]["component"]
            .as_str()
            .or_else(|| ctx.metadata["component"].as_str())
            .unwrap_or(&ctx.artifact_id);
        let new_version = ctx.metadata["evaluation"]["new_version"]
            .as_str()
            .or_else(|| ctx.metadata["new_version"].as_str())
            .unwrap_or("v0.0.0");
        let recommendation = ctx.metadata["recommendation"].as_str().unwrap_or("hold");
        let overall_score = ctx.metadata["overall_score"].as_f64().unwrap_or(0.0);

        info!(
            component,
            new_version,
            recommendation = %recommendation,
            score = %overall_score,
            run_id = %ctx.run_id,
            "skill-manage agent: self-upgrade lifecycle decision"
        );

        if recommendation == "discard" || overall_score < ACTIVATION_THRESHOLD {
            info!(
                component,
                new_version,
                "self-upgrade rejected (below threshold or recommendation=discard)"
            );
            return Ok(json!({
                "build_type": "self_upgrade",
                "action": "discarded",
                "component": component,
                "new_version": new_version,
                "artifact_id": ctx.artifact_id,
                "reason": format!(
                    "score {overall_score:.2} below threshold {ACTIVATION_THRESHOLD} or recommendation=discard"
                ),
            }));
        }

        info!(
            component,
            new_version,
            action = "activated",
            "self-upgrade approved — king will trigger update.sh"
        );

        Ok(json!({
            "build_type": "self_upgrade",
            "action": "activated",
            "component": component,
            "new_version": new_version,
            "artifact_id": ctx.artifact_id,
            "overall_score": overall_score,
        }))
    }
}
