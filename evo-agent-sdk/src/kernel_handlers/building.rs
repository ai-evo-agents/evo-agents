use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::handler::{AgentHandler, PipelineContext};
use crate::self_upgrade;

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Default handler for the **Building** kernel agent.
///
/// Two modes:
/// - **Skill build** (default): Packages a discovered skill into `manifest.toml`
///   + `config.toml` by querying the LLM via the gateway.
/// - **Self-upgrade build** (`build_type: "self_upgrade"`): Pulls source, runs
///   `cargo build --release`, packages the binary, and publishes a GitHub release.
pub struct BuildingHandler;

#[async_trait]
impl AgentHandler for BuildingHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        if self_upgrade::is_self_upgrade(&ctx.metadata) {
            return self.build_upgrade(&ctx).await;
        }

        self.build_skill(&ctx).await
    }
}

impl BuildingHandler {
    /// Original skill packaging via LLM.
    async fn build_skill(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        info!(artifact_id = %ctx.artifact_id, "building agent: packaging skill");

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
            serde_json::to_string_pretty(&ctx.metadata).unwrap_or_default()
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(2048),
            )
            .await?;

        let build_output = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "raw_response": response }));

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
            "artifact_id": ctx.artifact_id,
        }))
    }

    /// Self-upgrade: build component from source and publish release.
    async fn build_upgrade(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
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
            "building agent: self-upgrade build"
        );

        let result = self_upgrade::build_and_release(component, new_version).await?;

        info!(
            component,
            new_version,
            archive = %result.archive_path,
            "self-upgrade build complete"
        );

        Ok(json!({
            "build_type": "self_upgrade",
            "component": result.component,
            "new_version": result.new_version,
            "archive_path": result.archive_path,
            "binary_name": result.binary_name,
            "release_url": result.release_url,
            "artifact_id": ctx.artifact_id,
        }))
    }
}
