use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::handler::{AgentHandler, PipelineContext};
use crate::health_check;
use crate::self_upgrade;

/// Default handler for the **Pre-load** kernel agent.
///
/// Two modes:
/// - **Skill pre-load** (default): Health-checks skill API endpoints.
///   Does NOT use the LLM — purely endpoint validation.
/// - **Self-upgrade pre-load** (`build_type: "self_upgrade"`): Downloads
///   the release archive, extracts, and validates structure + binary health.
pub struct PreLoadHandler;

#[async_trait]
impl AgentHandler for PreLoadHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        if self_upgrade::is_self_upgrade(&ctx.metadata) {
            return self.validate_upgrade(&ctx).await;
        }

        self.check_endpoints(&ctx).await
    }
}

impl PreLoadHandler {
    /// Original endpoint health-checking.
    async fn check_endpoints(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        info!(artifact_id = %ctx.artifact_id, "pre-load agent: health-checking endpoints");

        // Extract endpoint URLs from build output config
        let mut urls_to_check = Vec::new();

        if let Some(config_str) = ctx.metadata["build_output"]["config_toml"].as_str()
            && let Ok(config) = toml::from_str::<evo_common::skill::SkillConfig>(config_str)
        {
            for endpoint in &config.endpoints {
                urls_to_check.push(endpoint.url.clone());
            }
        }

        // Also check any URLs in the metadata directly
        if let Some(endpoints) = ctx.metadata["endpoints"].as_array() {
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

        info!(checked = results.len(), "all endpoints healthy");

        Ok(json!({
            "health_results": health_json,
            "all_healthy": all_healthy,
        }))
    }

    /// Self-upgrade: validate the release archive.
    async fn validate_upgrade(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        let component = ctx.metadata["component"]
            .as_str()
            .unwrap_or(&ctx.artifact_id);
        let new_version = ctx.metadata["new_version"]
            .as_str()
            .unwrap_or("v0.0.0");
        let archive_path = ctx.metadata["archive_path"]
            .as_str()
            .or_else(|| ctx.metadata["release_url"].as_str())
            .unwrap_or("");

        info!(
            component,
            new_version,
            run_id = %ctx.run_id,
            "pre-load agent: validating self-upgrade release"
        );

        let result = self_upgrade::validate_release(
            component,
            new_version,
            archive_path,
        ).await?;

        if !result.all_passed {
            return Err(anyhow::anyhow!(
                "Self-upgrade validation failed for {component} {new_version}: \
                 binary_exists={}, executable={}, soul_md={}, health={}",
                result.binary_exists,
                result.binary_executable,
                result.soul_md_exists,
                result.health_check_passed,
            ));
        }

        Ok(json!({
            "build_type": "self_upgrade",
            "component": component,
            "new_version": new_version,
            "validation": {
                "binary_exists": result.binary_exists,
                "binary_executable": result.binary_executable,
                "soul_md_exists": result.soul_md_exists,
                "skills_dir_exists": result.skills_dir_exists,
                "health_check_passed": result.health_check_passed,
                "all_passed": result.all_passed,
            },
            "artifact_id": ctx.artifact_id,
        }))
    }
}
