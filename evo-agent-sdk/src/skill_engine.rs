use anyhow::{Context, Result};
use evo_common::skill::{SkillConfig, SkillManifest};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

// ─── Skill discovery ──────────────────────────────────────────────────────────

/// Represents a single loaded skill in the agent's `skills/` directory.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    pub name: String,
    pub manifest: SkillManifest,
    pub config: Option<SkillConfig>,
    pub path: PathBuf,
}

/// Scan `<agent_dir>/skills/` and load all valid skill manifests.
pub fn load_skills(agent_dir: &Path) -> Vec<LoadedSkill> {
    let skills_dir = agent_dir.join("skills");

    let entries = match std::fs::read_dir(&skills_dir) {
        Ok(e) => e,
        Err(_) => {
            info!("no skills/ directory found — agent has no pre-loaded skills");
            return vec![];
        }
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| load_skill(&e.path()).ok())
        .collect()
}

fn load_skill(skill_dir: &Path) -> Result<LoadedSkill> {
    let manifest_path = skill_dir.join("manifest.toml");
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: SkillManifest = toml::from_str(&manifest_str)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;

    let config = read_skill_config(skill_dir);

    let name = manifest.name.clone();
    info!(skill = %name, path = %skill_dir.display(), "loaded skill");

    Ok(LoadedSkill {
        name,
        manifest,
        config,
        path: skill_dir.to_path_buf(),
    })
}

fn read_skill_config(skill_dir: &Path) -> Option<SkillConfig> {
    let config_path = skill_dir.join("config.toml");
    if !config_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    toml::from_str(&content).ok()
}

// ─── Skill execution ──────────────────────────────────────────────────────────

/// Execute a config-only skill by making HTTP calls defined in its config.
pub async fn run_config_skill(
    client: &reqwest::Client,
    skill: &LoadedSkill,
    input: &serde_json::Value,
) -> Result<serde_json::Value> {
    let config = skill
        .config
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Skill '{}' has no config.toml", skill.name))?;

    if config.endpoints.is_empty() {
        return Ok(serde_json::json!({ "status": "no_endpoints" }));
    }

    // For now execute the first endpoint (extend in future phases)
    let endpoint = &config.endpoints[0];
    info!(skill = %skill.name, url = %endpoint.url, "calling skill endpoint");

    let mut req = client.post(&endpoint.url).json(input);

    // Inject API key if auth_ref is set
    if let Some(auth_ref) = &config.auth_ref {
        if let Ok(key) = std::env::var(auth_ref) {
            req = req.bearer_auth(key);
        } else {
            warn!(auth_ref = %auth_ref, "auth env var not set for skill");
        }
    }

    let resp = req.send().await.context("Skill HTTP request failed")?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_else(|_| serde_json::json!({}));

    if !status.is_success() {
        anyhow::bail!("Skill endpoint returned {status}: {body}");
    }

    Ok(body)
}
