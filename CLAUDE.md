# evo-agents

Runner binary + kernel and user agent scaffolding for the evo self-evolution system.

## Quick Commands

```bash
# Build the runner binary
cargo build -p runner

# Build release
cargo build -p runner --release

# Run a specific agent (e.g. the learning kernel agent)
cargo run -p runner -- kernel/learning

# Run with king address override
KING_ADDRESS=http://localhost:3000 cargo run -p runner -- kernel/learning

# Run tests
cargo test -p runner

# Lint
cargo clippy -p runner -- -D warnings
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `KING_ADDRESS` | `http://localhost:3000` | evo-king Socket.IO server URL |
| `AGENT_FOLDER` | `.` | Fallback agent dir (used if no CLI arg given) |
| `EVO_LOG_DIR` | `./logs` | Log output directory |
| `RUST_LOG` | `info` | Log level filter |

## Workspace Structure

```
evo-agents/
├── Cargo.toml           — workspace root (members: ["runner"])
├── runner/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs          — entry: load soul, connect Socket.IO, heartbeat loop
│       ├── soul.rs          — parse soul.md → Soul { role, agent_id, body }
│       ├── skill_engine.rs  — discover + execute skills from skills/ dir
│       ├── health_check.rs  — probe API endpoints, format for agent:health
│       └── event_handler.rs — role-based dispatch of king:command / pipeline:next
├── kernel/
│   ├── learning/
│   ├── building/
│   ├── pre-load/
│   ├── evaluation/
│   └── skill-manage/
└── users/
    └── .gitkeep
```

## Agent Folder Layout

Each agent (kernel or user) is a directory containing:

```
<agent-name>/
├── soul.md           — Identity, role, behavior description, event spec
├── skills/           — Skill subdirs (each has manifest.toml + optional config.toml)
│   └── <skill-name>/
│       ├── manifest.toml
│       └── config.toml
├── mcp/              — MCP server configs (future)
├── api-key.config    — API keys for this agent (gitignored)
└── download-runner.sh -> ../../download-runner.sh  (symlink)
```

## soul.md Format

```markdown
# <Agent Name>

## Role
<one-line role description>

## Behavior
- <behavior bullet 1>
- <behavior bullet 2>

## Events
- pipeline:next (stage=<role>) → <what to do>
- king:command (<cmd>) → <what to do>
```

The runner reads `## Role` to identify itself. The `agent_id` is derived as `<role>-<uuid4>`.

## Skill Files

### `manifest.toml`
```toml
name = "my-skill"
version = "0.1.0"
capabilities = ["search", "fetch"]

[inputs]
query = { type = "string", description = "Search query" }

[outputs]
results = { type = "array", description = "List of results" }
```

### `config.toml` (config-only skill — HTTP endpoints)
```toml
auth_ref = "MY_API_KEY"   # env var name for bearer auth

[[endpoints]]
url = "https://api.example.com/search"
method = "POST"
```

## Kernel Pipeline

The 5 kernel agents form a self-evolution pipeline:

| Stage | Role | Responsibility |
|-------|------|---------------|
| 1 | `learning` | Discover candidate skills from external sources |
| 2 | `building` | Package skill artifacts (manifest.toml + config.toml) |
| 3 | `pre-load` | Health-check all skill API endpoints before evaluation |
| 4 | `evaluation` | Score skills: correctness 40%, latency 25%, cost 20%, reliability 15% |
| 5 | `skill-manage` | Activate/deactivate skills based on evaluation scores |

Pipeline flow triggered by king via `pipeline:next` events.

## Socket.IO Protocol

Runner is a **client** connecting to king's Socket.IO server.

### Emits (runner → king)

| Event | Payload | When |
|-------|---------|------|
| `agent:register` | `{ agent_id, role, capabilities }` | On connect |
| `agent:status` | `{ agent_id, status }` | Every 30 s (heartbeat) |
| `agent:skill_report` | `{ agent_id, skill_id, result, score }` | After skill evaluation |
| `agent:health` | `{ agent_id, health_checks: [...] }` | After pre-load health run |

### Receives (king → runner)

| Event | Description |
|-------|-------------|
| `king:command` | Execute a targeted command (role-dependent) |
| `pipeline:next` | Advance to next pipeline stage with an artifact |

See `evo-common/src/messages.rs` for full type definitions.

## Download Runner Script

`download-runner.sh` — platform auto-detection script symlinked into each kernel agent folder:

```bash
cd kernel/learning
./download-runner.sh   # downloads the correct evo-runner binary for this platform
```

Platforms: `linux-x86_64`, `linux-aarch64`, `macos-x86_64` (Intel), `macos-arm64` (Apple Silicon), `windows-x86_64`.

Downloads from: `https://github.com/ai-evo-agents/evo-agents/releases/latest`

## Logging

Logs write to `logs/<role>.log` (e.g. `logs/learning.log`) and stdout.

```bash
RUST_LOG=debug cargo run -p runner -- kernel/learning
```

Log file is named after the agent's role (`## Role` in soul.md), written in JSON format.
