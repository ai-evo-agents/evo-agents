# evo-agents

Runner binary source for the Evo self-evolution agent system. This is a Cargo workspace under the `ai-evo-agents` GitHub organization containing the `evo-runner` binary crate.

## Current Status

Active development — runner binary with Socket.IO client, health checks, and role-based event dispatch.

## Part of the Evo System

| Crate | Role |
|-------|------|
| evo-common | Shared types used across all crates (published on crates.io) |
| evo-gateway | API aggregator; agents call external APIs through it |
| evo-king | Orchestrator; runners connect to it via Socket.IO |
| **evo-agents** | Runner binary (this repo) |
| evo-kernel-agent-* | Kernel agent data repos (soul.md, skills/, mcp/) |
| evo-user-agent-template | Template for creating user agents |

## Workspace Structure

```
evo-agents/
├── Cargo.toml              # Workspace root
├── runner/                  # Runner binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # Entry: parse agent folder, load soul.md, connect socket.io
│       ├── soul.rs          # Parse soul.md for role and behavior
│       ├── event_handler.rs # Role-based event dispatch
│       ├── skill_engine.rs  # Execute skills (config or code)
│       └── health_check.rs  # Endpoint health testing
├── download-runner.sh       # Platform-aware binary downloader
├── publish.sh               # Build, validate, commit, push, tag
└── users/                   # Legacy; user agents now use evo-user-agent-template
    └── .gitkeep
```

## Runner Architecture

The runner is a **generic binary** that can operate as any agent role. Each agent instance runs the same `evo-runner` binary, pointed at an agent folder. The runner loads the agent's identity from `soul.md`, connects to king via Socket.IO, and dispatches events based on role.

### How Agents Use the Runner

Kernel and user agents live in their own repos (`evo-kernel-agent-*`, `evo-user-agent-*`). Each agent repo includes a `download-runner.sh` script that fetches the pre-built `evo-runner` binary from this repo's GitHub Releases.

```sh
# In any agent repo:
./download-runner.sh          # Downloads evo-runner for current platform
./evo-runner .                # Runs agent with current directory as agent folder
```

### Source Files

**`main.rs`**

Entry point. Parses the agent folder path from the CLI argument, loads `soul.md` to determine the agent's role, connects to king's Socket.IO server, registers with an `AgentRegister` message, and starts the event loop. Skills and capabilities are included in the `agent:register` payload. After registration, the runner performs a health check against king's `/health` HTTP endpoint and emits the results to king via `agent:health`.

**`soul.rs`**

Parses `soul.md` to extract agent identity. Returns a `Soul` struct with `agent_id`, `role`, `behavior`, and raw `body`. The `## Role` header determines event handler registration; `## Behavior` provides context for LLM-powered processing.

**`event_handler.rs`**

Role-based event dispatch. Reads the role from the `## Role` header in `soul.md` and registers the appropriate event handlers:

- `Learning` - handles `pipeline:next(stage=learning)`, `king:command(discover)`
- `Building` - handles `pipeline:next(stage=building)`, `king:command(build)`
- `Pre-load` - handles `pipeline:next(stage=pre_load)`, runs health checks
- `Evaluation` - handles `pipeline:next(stage=evaluation)`, `king:command(evaluate)`
- `Skill Manage` - handles `pipeline:next(stage=skill_manage)`, `king:command(activate/deactivate)`

All roles also handle `king:command` generically.

**`skill_engine.rs`**

Executes skills. Parses `manifest.toml` using `evo_common::skill::SkillManifest` to determine skill type:

- Config-only skills (`has_code = false`): makes HTTP API calls as defined in `config.toml` via `evo_common::skill::SkillConfig`
- Code skills (`has_code = true`): executes a Rust module or external script

Reports results to king via `agent:skill_report`.

**`health_check.rs`**

Endpoint health testing. For each URL, checks reachability, measures latency (ms), and records HTTP status code. Used both for post-connect king health checks and pre-load skill validation.

## Health Check on Connect

After connecting to king and sending `agent:register`, the runner automatically verifies HTTP connectivity:

1. Builds an HTTP client with a 5-second timeout.
2. Sends `GET {KING_ADDRESS}/health` to probe king's HTTP endpoint.
3. Records reachability, latency (ms), and HTTP status code.
4. Emits `agent:health` event to king with the health check results.
5. Logs whether the health check passed or failed.

This ensures the runner can reach king via both Socket.IO (for events) and HTTP (for health probes) before entering its heartbeat loop.

## Registration Payload

```json
{
    "agent_id": "learning-learning",
    "role": "learning",
    "capabilities": ["discover", "evaluate"],
    "skills": ["web-search", "summarize"]
}
```

Capabilities are aggregated from all loaded skill manifests (deduplicated), and skills lists the names of all loaded skills.

## Agent Folder Structure

Every agent repo follows the same layout:

```
agent-repo/
├── soul.md              # Agent identity, role, behavior rules
├── skills/              # Skills this agent can use
│   └── web-discovery/
│       ├── manifest.toml   # Capabilities, I/O, dependencies
│       ├── config.toml     # API endpoints, auth refs
│       └── src/            # Optional custom code
├── mcp/                 # MCP server definitions
├── api-key.config       # API key references (gitignored)
└── download-runner.sh   # Script to download evo-runner for current platform
```

## Kernel Agent Repos

Five kernel agents form the evolution pipeline. Each lives in its own repo:

| Agent | Repo | Role |
|-------|------|------|
| Learning | [evo-kernel-agent-learning](https://github.com/ai-evo-agents/evo-kernel-agent-learning) | Discover potential new skills |
| Building | [evo-kernel-agent-building](https://github.com/ai-evo-agents/evo-kernel-agent-building) | Package skills into artifacts |
| Pre-load | [evo-kernel-agent-pre-load](https://github.com/ai-evo-agents/evo-kernel-agent-pre-load) | Health-check skill endpoints |
| Evaluation | [evo-kernel-agent-evaluation](https://github.com/ai-evo-agents/evo-kernel-agent-evaluation) | Score and benchmark skills |
| Skill Manage | [evo-kernel-agent-skill-manage](https://github.com/ai-evo-agents/evo-kernel-agent-skill-manage) | Activate, hold, or discard |

Pipeline flow:

```
Learning -> Building -> Pre-load -> Evaluation -> Skill Manage -> (repeats)
```

## User Agents

User agents are created from the [evo-user-agent-template](https://github.com/ai-evo-agents/evo-user-agent-template). Each user agent gets its own repo following the same folder structure. They are spawned with custom roles via `AgentRole::User(String)`.

## soul.md Format

Each agent folder contains a `soul.md` file that defines the agent's identity, role, and behavior:

```markdown
# Learning Agent

## Role
learning

## Behavior
- Monitor configured skill sources (registries, APIs, community feeds)
- Evaluate discovered skills against system needs
- Report findings to king via agent:skill_report
- Trigger pipeline for promising skills

## Events
- pipeline:next (stage=learning) -> Start discovery task
- king:command (discover) -> Targeted skill search
```

The runner reads `## Role` for event handler dispatch and `## Behavior` for LLM system prompts.

## Skill Manifest Format

`manifest.toml` defines a skill's capabilities, inputs, outputs, and whether it includes custom code:

```toml
name = "web-search"
version = "0.1.0"
description = "Search the web for information"
capabilities = ["search", "summarize"]
has_code = false
dependencies = []

[[inputs]]
name = "query"
type = "string"
required = true
description = "Search query"

[[outputs]]
name = "results"
type = "array"
required = true
description = "Search results"
```

## Skill Config Format

`config.toml` defines the API endpoints and authentication references for config-only skills:

```toml
auth_ref = "SEARCH_API_KEY"

[[endpoints]]
name = "search"
url = "https://api.search.com/v1/search"
method = "GET"

[endpoints.headers]
Accept = "application/json"
```

The `auth_ref` field names an environment variable or secret reference rather than storing a key directly.

## download-runner.sh

Each agent repo includes a `download-runner.sh` script that fetches the correct pre-built runner binary for the current platform from GitHub Releases:

- Detects OS (`linux`, `darwin`, `windows`) and architecture (`x86_64`, `aarch64`)
- Downloads from `https://github.com/ai-evo-agents/evo-agents/releases/latest`
- Makes the binary executable and places it as `./evo-runner`

## CI and Cross-Platform Builds

The GitHub Actions workflow builds the runner binary for five targets on every tag push:

| Target | Platform |
|--------|----------|
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows x86_64 |

Release artifacts are uploaded to the GitHub Release and are what `download-runner.sh` fetches.

## Building and Running

Build the runner binary:

```sh
cargo build -p runner --release
```

Run an agent by passing its agent repo path:

```sh
./target/release/evo-runner ../evo-kernel-agent-learning
```

Or using cargo:

```sh
cargo run -p runner -- ../evo-kernel-agent-learning
```

The runner reads `soul.md` from the provided folder, connects to king, and begins processing events.

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| rust_socketio | 0.6 (async) | Socket.IO client for king connection |
| tokio | 1 (full) | Async runtime |
| reqwest | 0.12 (json) | HTTP client for skill API calls |
| serde | 1.0 | Serialization framework |
| serde_json | 1.0 | JSON serialization |
| toml | 0.8 | TOML parsing for manifests and configs |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Log output formatting |
| evo-common | 0.1 (crates.io) | Shared types (SkillManifest, SkillConfig, AgentRole, etc.) |

## License

MIT
