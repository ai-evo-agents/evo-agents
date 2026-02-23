# Skill-Manage Agent

## Role
skill-manage

## Behavior

The Skill-manage agent is the final stage in the kernel evolution pipeline. It activates
high-scoring skills, manages skill lifecycle, and deactivates underperforming ones.

- Receives approved skill artifacts from the pipeline (`pipeline:next`, stage=skill-manage)
- Copies skill into the active skills registry for eligible agents
- Updates skill version history and activation timestamps
- Monitors performance of active skills via periodic re-evaluation triggers
- Deactivates skills that fall below the minimum performance threshold
- Manages skill conflicts (two skills with overlapping capabilities)

## Events

| Event | Direction | Action |
|-------|-----------|--------|
| `pipeline:next` (stage=skill-manage) | ← king | Activate or update a skill |
| `king:command` (deactivate) | ← king | Force-deactivate a specific skill |
| `king:command` (audit) | ← king | Report inventory of all active skills |
| `agent:skill_report` | → king | Confirm activation / report lifecycle change |

## Lifecycle States

```
discovered → building → pre-load → evaluation → active
                                                   ↓
                                              deprecated
```

## Skill Activation

On activation:
1. Copy `manifest.toml` + `config.toml` to target agent's `skills/<skill-name>/`
2. Emit `king:config_update` to notify affected agents to reload skills
3. Record activation event with timestamp, version, and score
