# evo-agents

Runner binary source and agent data folders for the Evo self-evolution agent system. This is a Cargo workspace under the `ai-evo-agents` GitHub organization containing the `runner` binary crate and all kernel/user agent folder definitions.

## Current Status

Skeleton - Phase 5 pending implementation.

## Part of the Evo System

| Crate | Role |
|-------|------|
| evo-common | Shared types used across all crates |
| evo-gateway | API aggregator; agents call external APIs through it |
| evo-king | Orchestrator; runners connect to it via Socket.IO |
| **evo-agents** | Runner binary + agent data folders (this repo) |

## Workspace Structure

```
evo-agents/
├── Cargo.toml              # Workspace root
├── runner/                  # Runner binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          # Entry: parse agent folder, load soul.md, connect socket.io
│       ├── event_handler.rs # Role-based event dispatch
│       ├── skill_engine.rs  # Execute skills (config or code)
│       ├── health_check.rs  # Pre-load API health testing
│       └── socket_client.rs # rust-socketio client to king
├── kernel/                  # 5 kernel agents (evolution pipeline)
│   ├── skill-manage/
│   ├── learning/
│   ├── pre-load/
│   ├── building/
│   └── evaluation/
└── users/                   # Dynamically created by king/kernel agents
    └── .gitkeep
```

## Runner Architecture

Each agent instance runs the same `runner` binary, pointed at its agent folder. The runner loads the agent's identity from `soul.md`, connects to king via Socket.IO, and dispatches events based on role.

### Source Files

**`main.rs`**

Entry point. Parses the agent folder path from the CLI argument, loads `soul.md` to determine the agent's role, connects to king's Socket.IO server, registers with an `AgentRegister` message, and starts the event loop. Skills and capabilities are now included in the `agent:register` payload (previously sent empty capabilities). After registration, the runner performs a health check against king's `/health` HTTP endpoint and emits the results to king via `agent:health`.

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

Pre-load API health testing. For each skill, checks that configured API endpoints are reachable, verifies authentication is valid, and measures latency. Reports `HealthCheck` results to king via `agent:health`.

**`socket_client.rs`**

Manages the Socket.IO connection to king. Reads the server address from `AgentConfig.king_address`, emits `AgentRegister` on connect, sends periodic heartbeats via `AgentStatus`, and handles disconnect and reconnect. The registration payload now includes capabilities and skills:

```json
{
    "agent_id": "learning-learning",
    "role": "learning",
    "capabilities": ["discover", "evaluate"],
    "skills": ["web-search", "summarize"]
}
```

Capabilities are aggregated from all loaded skill manifests (deduplicated), and skills lists the names of all loaded skills.

## Health Check on Connect

After connecting to king and sending `agent:register`, the runner automatically verifies HTTP connectivity:

1. Builds an HTTP client with a 5-second timeout.
2. Sends `GET {KING_ADDRESS}/health` to probe king's HTTP endpoint.
3. Records reachability, latency (ms), and HTTP status code.
4. Emits `agent:health` event to king with the health check results.
5. Logs whether the health check passed or failed.

This ensures the runner can reach king via both Socket.IO (for events) and HTTP (for health probes) before entering its heartbeat loop.

## Agent Folder Structure

Every agent, whether kernel or user, follows the same folder layout:

```
kernel/learning/
├── soul.md              # Agent identity, role, behavior rules
├── skills/              # Skills this agent can use
│   └── web-discovery/
│       ├── manifest.toml   # Capabilities, I/O, dependencies
│       ├── config.toml     # API endpoints, auth refs
│       └── src/            # Optional custom code
├── mcp/                 # MCP server definitions for this agent
├── api-key.config       # API key references (gitignored)
├── runner               # Built runner binary (from CI, gitignored)
└── download-runner.sh   # Script to download runner for current platform
```

## Kernel Agents

Five kernel agents form a continuous improvement cycle - the evolution pipeline:

**1. Learning**

Discovers potential new skills from external sources such as registries, APIs, and community feeds. Evaluates discovered skills against current system needs and reports findings to king.

**2. Building**

Packages and compiles discovered skills. Creates `manifest.toml` and `config.toml` for each skill and bundles optional custom code.

**3. Pre-load**

Validates skills before activation. Health-checks all API endpoints, verifies authentication credentials, and tests connectivity.

**4. Evaluation**

Tests skill quality and performance. Scores skills and benchmarks them against existing capabilities in the system.

**5. Skill Manage**

Final decision maker. Activates, holds, or discards skills based on evaluation results. Manages the skill inventory across all agents.

Pipeline flow:

```
Learning -> Building -> Pre-load -> Evaluation -> Skill Manage -> (repeats)
```

## User Agents

User agents are created dynamically by the king process or by kernel agents based on discovered needs. Each user agent gets its own folder under `users/` following the same structure as kernel agents. They are spawned with custom roles via `AgentRole::User(String)`.

## soul.md Format

Each agent folder contains a `soul.md` file that defines the agent's identity, role, and behavior:

```markdown
# Learning Agent

## Role
Discover potential new skills from external sources.

## Behavior
- Monitor configured skill sources (registries, APIs, community feeds)
- Evaluate discovered skills against system needs
- Report findings to king via agent:skill_report
- Trigger pipeline for promising skills

## Events
- pipeline:next (stage=learning) -> Start discovery task
- king:command (discover) -> Targeted skill search
```

The runner reads the `## Role` header to determine which event handlers to register.

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

Each agent folder includes a `download-runner.sh` script that fetches the correct pre-built runner binary for the current platform from GitHub Releases:

- Detects OS (`linux`, `darwin`, `windows`) and architecture (`x86_64`, `aarch64`)
- Downloads from `https://github.com/ai-evo-agents/evo-agents/releases/latest`
- Makes the binary executable and places it as `./runner`

This means agent folders can be deployed without a local Rust toolchain; running `download-runner.sh` is sufficient to get a working agent binary.

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

Run an agent by passing its folder path as the first argument:

```sh
./target/release/runner ./kernel/learning
```

Or using cargo:

```sh
cargo run -p runner -- ./kernel/learning
```

The runner reads `soul.md` from the provided folder, connects to king, and begins processing events.

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| rust_socketio | 0.7 (async) | Socket.IO client for king connection |
| tokio | 1 (full) | Async runtime |
| reqwest | 0.12 (json) | HTTP client for skill API calls |
| serde | 1.0 | Serialization framework |
| serde_json | 1.0 | JSON serialization |
| toml | 0.8 | TOML parsing for manifests and configs |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Log output formatting |
| evo-common | git dep | Shared types (SkillManifest, SkillConfig, AgentRole, etc.) |

## License

MIT
