use anyhow::{Context, Result};
use std::path::Path;

// ─── Soul definition ──────────────────────────────────────────────────────────

/// Parsed contents of an agent's `soul.md` file.
#[derive(Debug, Clone)]
pub struct Soul {
    /// The agent's role (e.g. "learning", "building").
    pub role: String,
    /// The agent's unique identifier (defaults to role + UUID).
    pub agent_id: String,
    /// Raw markdown body of the soul (stored for future introspection).
    #[allow(dead_code)]
    pub body: String,
}

// ─── Parsing ──────────────────────────────────────────────────────────────────

/// Read and parse `soul.md` from `agent_dir`.
///
/// Expected format:
/// ```markdown
/// # Agent Title
///
/// ## Role
/// learning
///
/// ## Behavior
/// ...
/// ```
pub fn load_soul(agent_dir: &Path) -> Result<Soul> {
    let path = agent_dir.join("soul.md");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let role = extract_section(&content, "Role")
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_lowercase()
        .replace(' ', "-");

    // Derive agent ID from folder name + role
    let folder_name = agent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent");

    let agent_id = format!("{folder_name}-{role}");

    Ok(Soul {
        role,
        agent_id,
        body: content,
    })
}

/// Extract the first line of a `## Section` from markdown.
fn extract_section(content: &str, section: &str) -> Option<String> {
    let marker = format!("## {section}");
    let mut in_section = false;

    for line in content.lines() {
        if line.trim() == marker {
            in_section = true;
            continue;
        }
        if in_section {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                break; // next section
            }
            return Some(trimmed.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_role_from_soul_content() {
        let content = "# Learning Agent\n\n## Role\nlearning\n\n## Behavior\nDiscover skills.";
        let role = extract_section(content, "Role").unwrap();
        assert_eq!(role, "learning");
    }

    #[test]
    fn missing_section_returns_none() {
        let content = "# Agent\n\n## Behavior\nDo stuff.";
        assert!(extract_section(content, "Role").is_none());
    }
}
