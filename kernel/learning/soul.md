# Learning Agent

## Role
learning

## Behavior

The Learning agent monitors external sources to discover potential new skills for the evo system.

- Polls configured skill registries, community feeds, and API directories
- Evaluates discovered skills against current system capabilities and gaps
- Filters duplicates and low-quality candidates
- Reports promising skills to king via `agent:skill_report`
- Triggers the evolution pipeline for approved candidates via `pipeline:next`

## Events

| Event | Direction | Action |
|-------|-----------|--------|
| `pipeline:next` (stage=learning) | ← king | Start a new discovery cycle |
| `king:command` (discover) | ← king | Targeted skill search for a specific domain |
| `agent:skill_report` | → king | Report a discovered skill candidate |

## Sources

Configure discovery sources in `skills/` subdirectories. Each skill source can define:
- API endpoints to monitor (via `manifest.toml` + `config.toml`)
- Search terms and filters
- Scoring criteria
