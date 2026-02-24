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
    /// The `## Behavior` section content — used as the LLM system prompt.
    pub behavior: String,
    /// Raw markdown body of the soul (stored for future introspection).
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

    let behavior = extract_full_section(&content, "Behavior").unwrap_or_default();

    // Derive agent ID from folder name + role
    let folder_name = agent_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("agent");

    let agent_id = format!("{folder_name}-{role}");

    Ok(Soul {
        role,
        agent_id,
        behavior,
        body: content,
    })
}

/// Extract the first non-empty line of a `## Section` from markdown.
pub fn extract_section(content: &str, section: &str) -> Option<String> {
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

/// Extract the full multi-line content of a `## Section` from markdown.
///
/// Returns all lines between `## Section` and the next `##` header (or EOF).
pub fn extract_full_section(content: &str, section: &str) -> Option<String> {
    let marker = format!("## {section}");
    let mut in_section = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if line.trim() == marker {
            in_section = true;
            continue;
        }
        if in_section {
            if line.trim().starts_with("## ") {
                break; // next section
            }
            lines.push(line);
        }
    }

    if lines.is_empty() {
        return None;
    }

    // Trim leading/trailing empty lines
    let text = lines.join("\n");
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
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

    #[test]
    fn extract_full_behavior_section() {
        let content = "# Learning Agent\n\n## Role\nlearning\n\n## Behavior\n- Discover skills\n- Evaluate candidates\n- Report findings\n\n## Events\n- pipeline:next";
        let behavior = extract_full_section(content, "Behavior").unwrap();
        assert!(behavior.contains("Discover skills"));
        assert!(behavior.contains("Evaluate candidates"));
        assert!(behavior.contains("Report findings"));
        assert!(!behavior.contains("pipeline:next")); // should not include next section
    }

    #[test]
    fn extract_full_section_at_end_of_file() {
        let content = "# Agent\n\n## Role\ntest\n\n## Behavior\nDo stuff.\nMore stuff.";
        let behavior = extract_full_section(content, "Behavior").unwrap();
        assert!(behavior.contains("Do stuff."));
        assert!(behavior.contains("More stuff."));
    }
}
