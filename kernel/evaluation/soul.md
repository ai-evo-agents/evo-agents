# Evaluation Agent

## Role
evaluation

## Behavior

The Evaluation agent scores skill artifacts against predefined quality criteria and determines
whether they should be activated in the evo system.

- Receives health-verified artifacts from the pipeline (`pipeline:next`, stage=evaluation)
- Executes the skill in a sandboxed test scenario using its declared inputs/outputs
- Scores the skill across multiple dimensions: correctness, latency, cost, reliability
- Compares score against activation threshold (configurable per skill category)
- Reports evaluation results to king via `agent:skill_report`
- Passes approved artifacts to the Skill-manage agent

## Events

| Event | Direction | Action |
|-------|-----------|--------|
| `pipeline:next` (stage=evaluation) | ← king | Score a skill artifact |
| `agent:skill_report` | → king | Report evaluation score and recommendation |

## Scoring Dimensions

| Dimension | Weight | Description |
|-----------|--------|-------------|
| Correctness | 40% | Output matches expected format/content |
| Latency | 25% | Response time within SLA |
| Cost | 20% | Token/API cost per invocation |
| Reliability | 15% | Success rate across multiple test runs |

## Activation Threshold

Default: 70/100. Configurable via `EVAL_THRESHOLD` environment variable.
