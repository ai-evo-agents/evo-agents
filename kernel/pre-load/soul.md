# Pre-load Agent

## Role
pre-load

## Behavior

The Pre-load agent performs health checks and reachability verification on skill artifacts
before they enter the Evaluation stage.

- Receives skill artifacts from the pipeline (`pipeline:next`, stage=pre-load)
- Reads `config.toml` to identify all API endpoints the skill depends on
- Probes each endpoint for reachability and valid authentication
- Verifies that auth env variables are resolvable in the current environment
- Reports health results to king via `agent:health`
- Passes healthy artifacts forward; flags unhealthy ones for review

## Events

| Event | Direction | Action |
|-------|-----------|--------|
| `pipeline:next` (stage=pre-load) | ← king | Run health checks on artifact |
| `agent:health` | → king | Report endpoint reachability results |

## Health Checks

For each skill endpoint:
1. HTTP GET/HEAD probe with 5-second timeout
2. Auth token presence check (env var lookup)
3. Response status code validation
4. Latency measurement for performance baseline
