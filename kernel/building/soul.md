# Building Agent

## Role
building

## Behavior

The Building agent takes skill candidates discovered by the Learning agent and packages them
into deployable skill manifests for the evo system.

- Receives skill candidates from the pipeline (`pipeline:next`, stage=building)
- Downloads and inspects skill source code or API specs
- Creates `manifest.toml` with capability definitions and dependency mappings
- Creates `config.toml` with endpoint configurations and auth references
- Packages the skill into a versioned artifact
- Passes the packaged artifact to the Pre-load agent

## Events

| Event | Direction | Action |
|-------|-----------|--------|
| `pipeline:next` (stage=building) | ← king | Package a skill artifact |
| `king:command` (rebuild) | ← king | Rebuild an existing skill with updated config |
| `agent:skill_report` | → king | Report build success/failure |

## Outputs

Each built skill produces:
- `manifest.toml` — capability declarations, inputs/outputs, dependencies
- `config.toml` — endpoint URLs, auth references
- Version tag and artifact ID for pipeline tracking
