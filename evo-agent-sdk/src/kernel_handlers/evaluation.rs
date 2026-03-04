use async_trait::async_trait;
use serde_json::{Value, json};
use tracing::info;

use crate::handler::{
    AgentHandler, DecomposeContext, ErrorRecoveryContext, PipelineContext, TaskEvaluateContext,
};
use crate::self_upgrade;

const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Default handler for the **Evaluation** kernel agent.
///
/// Two modes:
/// - **Skill evaluation** (default): Scores and benchmarks a skill across
///   multiple dimensions using the LLM.
/// - **Self-upgrade evaluation** (`build_type: "self_upgrade"`): Compares
///   new version vs current, verifies all pre-load checks passed, and
///   produces a pass/fail verdict.
pub struct EvaluationHandler;

#[async_trait]
impl AgentHandler for EvaluationHandler {
    async fn on_pipeline(&self, ctx: PipelineContext<'_>) -> anyhow::Result<Value> {
        if self_upgrade::is_self_upgrade(&ctx.metadata) {
            return self.evaluate_upgrade(&ctx).await;
        }

        self.evaluate_skill(&ctx).await
    }

    async fn on_task_evaluate(&self, ctx: TaskEvaluateContext<'_>) -> anyhow::Result<Value> {
        // Skip pipeline tasks — those are handled by on_pipeline
        if ctx.task_type == "pipeline" {
            return Ok(Value::Null);
        }

        info!(task_id = %ctx.task_id, task_type = %ctx.task_type, "evaluating task output");

        let exit_info = match ctx.exit_code {
            Some(code) => format!("Exit code: {code}"),
            None => "No exit code (LLM prompt)".to_string(),
        };
        let latency_info = ctx
            .latency_ms
            .map(|ms| format!("Latency: {ms}ms"))
            .unwrap_or_default();

        let prompt = format!(
            "You are a task evaluator for an AI self-evolution system.\n\
             Evaluate the following task output and produce a brief summary.\n\n\
             Task type: {task_type}\n{exit_info}\n{latency_info}\n\n\
             Output (truncated):\n```\n{output}\n```\n\n\
             Respond with valid JSON containing:\n\
             - summary: 1-2 sentence summary of what happened\n\
             - score: 0.0-1.0 quality/success score\n\
             - tags: array of relevant tags\n\
             - learnings: any patterns or facts worth remembering",
            task_type = ctx.task_type,
            output = &ctx.output_summary[..ctx.output_summary.len().min(4000)],
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(512),
            )
            .await?;

        let evaluation = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "summary": response, "score": 0.5, "tags": [] }));

        Ok(json!({
            "summary": evaluation["summary"].as_str().unwrap_or("Task completed"),
            "score": evaluation["score"].as_f64().unwrap_or(0.5),
            "tags": evaluation.get("tags").cloned().unwrap_or(json!([])),
            "evaluation": evaluation,
        }))
    }

    async fn on_error_recovery(&self, ctx: ErrorRecoveryContext<'_>) -> anyhow::Result<Value> {
        info!(
            request_id = %ctx.request_id,
            run_id = %ctx.run_id,
            task_id = %ctx.task_id,
            failed_stage = %ctx.failed_stage,
            retry_count = %ctx.retry_count,
            "evaluation agent: analyzing pipeline failure"
        );

        let prompt = format!(
            "You are an error analyst for an AI self-evolution pipeline.\n\
             A pipeline stage has failed. Analyze the error and recommend ONE action.\n\n\
             Failed stage: {stage}\n\
             Error message: {error}\n\
             Retry count so far: {retry_count}\n\
             Task summary: {summary}\n\
             Stage output (truncated):\n```json\n{output}\n```\n\n\
             Available actions:\n\
             - retry: Retry the same stage (only if error seems transient, max 3 retries)\n\
             - decompose: Break the task into smaller subtasks that might succeed individually\n\
             - skip: Skip this stage and continue (only valid for evaluation and skill_manage)\n\
             - abort: Stop the pipeline entirely (use when error is fundamental)\n\n\
             Rules:\n\
             - If retry_count >= 3, do NOT recommend retry\n\
             - skip is only valid for evaluation and skill_manage stages\n\
             - If recommending decompose, include a 'subtasks' array in params\n\
             - If recommending retry, include any modified params in params\n\n\
             Respond with valid JSON only: {{ \"action\": \"...\", \"reasoning\": \"...\", \"params\": {{}} }}",
            stage = ctx.failed_stage,
            error = ctx.error_message,
            retry_count = ctx.retry_count,
            summary = ctx.task_summary,
            output = serde_json::to_string_pretty(&ctx.stage_output)
                .unwrap_or_default()
                .chars()
                .take(3000)
                .collect::<String>(),
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(512),
            )
            .await?;

        let result = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "action": "abort", "reasoning": response, "params": {} }));

        // Normalize action string
        let action = result["action"]
            .as_str()
            .unwrap_or("abort")
            .trim()
            .to_lowercase();

        // Enforce guardrails
        let final_action = match action.as_str() {
            "retry" if ctx.retry_count >= 3 => "abort",
            "skip" if ctx.failed_stage != "evaluation" && ctx.failed_stage != "skill_manage" => {
                "abort"
            }
            "retry" | "decompose" | "skip" | "abort" => &action,
            _ => "abort",
        };

        Ok(json!({
            "action": final_action,
            "reasoning": result["reasoning"].as_str().unwrap_or(""),
            "params": result.get("params").cloned().unwrap_or(json!({})),
        }))
    }

    async fn on_decompose(&self, ctx: DecomposeContext<'_>) -> anyhow::Result<Value> {
        info!(
            request_id = %ctx.request_id,
            task_id = %ctx.task_id,
            task_type = %ctx.task_type,
            trigger = %ctx.trigger,
            "evaluation agent: decomposing task"
        );

        let prompt = format!(
            "You are a task decomposition agent for an AI self-evolution system.\n\
             Analyze the following task and decide whether it should be broken into subtasks.\n\n\
             Task type: {task_type}\n\
             Summary: {summary}\n\
             Trigger: {trigger}\n\
             Payload (truncated):\n```json\n{payload}\n```\n\
             Context (truncated):\n```json\n{context}\n```\n\n\
             If the task is complex enough to benefit from decomposition, return subtasks.\n\
             Each subtask must have: task_type (string), summary (string), payload (object).\n\
             If the task is simple enough to execute directly, return empty subtasks.\n\n\
             Respond with valid JSON only:\n\
             {{ \"should_decompose\": true/false, \"reasoning\": \"...\", \"subtasks\": [...] }}",
            task_type = ctx.task_type,
            summary = ctx.summary,
            trigger = ctx.trigger,
            payload = serde_json::to_string_pretty(&ctx.payload)
                .unwrap_or_default()
                .chars()
                .take(2000)
                .collect::<String>(),
            context = serde_json::to_string_pretty(&ctx.context)
                .unwrap_or_default()
                .chars()
                .take(2000)
                .collect::<String>(),
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(1024),
            )
            .await?;

        let result = serde_json::from_str::<Value>(&response).unwrap_or_else(
            |_| json!({ "should_decompose": false, "subtasks": [], "reasoning": response }),
        );

        Ok(json!({
            "should_decompose": result["should_decompose"].as_bool().unwrap_or(false),
            "reasoning": result["reasoning"].as_str().unwrap_or(""),
            "subtasks": result.get("subtasks").cloned().unwrap_or(json!([])),
        }))
    }
}

impl EvaluationHandler {
    /// Original LLM-based skill evaluation.
    async fn evaluate_skill(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        info!(artifact_id = %ctx.artifact_id, "evaluation agent: scoring skill");

        let prompt = format!(
            "You are a skill evaluator for an AI self-evolution system.\n\
             Evaluate the following skill:\n\
             {}\n\n\
             Score it on these dimensions (0.0 to 1.0):\n\
             1. utility: How useful is this skill to the system?\n\
             2. reliability: How reliable are the endpoints/APIs?\n\
             3. novelty: Does it add genuinely new capabilities?\n\
             4. integration: How well does it fit with existing skills?\n\n\
             Also provide:\n\
             - overall_score: weighted average (utility=0.4, reliability=0.3, novelty=0.2, integration=0.1)\n\
             - recommendation: 'activate', 'hold', or 'discard'\n\
             - reasoning: brief explanation\n\
             - subtasks: an array of follow-up work items if recommendation is 'activate'.\n\
               Each subtask should have: task_type (string), summary (string), payload (object with relevant details).\n\
               Examples: integration testing, documentation, dependency check, configuration setup.\n\
               Return an empty array if no follow-up work is needed.\n\n\
             Respond with valid JSON.",
            serde_json::to_string_pretty(&ctx.metadata).unwrap_or_default()
        );

        let response = ctx
            .gateway
            .chat_completion(
                DEFAULT_MODEL,
                &ctx.soul.behavior,
                &prompt,
                Some(0.3),
                Some(1024),
            )
            .await?;

        let evaluation = serde_json::from_str::<Value>(&response)
            .unwrap_or_else(|_| json!({ "raw_response": response }));

        let overall_score = evaluation["overall_score"].as_f64().unwrap_or(0.0);
        let recommendation = evaluation["recommendation"]
            .as_str()
            .unwrap_or("hold")
            .to_string();

        info!(
            artifact_id = %ctx.artifact_id,
            overall_score = %overall_score,
            recommendation = %recommendation,
            "evaluation complete"
        );

        let subtasks = evaluation.get("subtasks").cloned().unwrap_or(json!([]));

        Ok(json!({
            "evaluation": evaluation,
            "artifact_id": ctx.artifact_id,
            "overall_score": overall_score,
            "recommendation": recommendation,
            "subtasks": subtasks,
        }))
    }

    /// Self-upgrade: evaluate the new release against current version.
    async fn evaluate_upgrade(&self, ctx: &PipelineContext<'_>) -> anyhow::Result<Value> {
        let component = ctx.metadata["component"]
            .as_str()
            .unwrap_or(&ctx.artifact_id);
        let new_version = ctx.metadata["new_version"].as_str().unwrap_or("v0.0.0");

        info!(
            component,
            new_version,
            run_id = %ctx.run_id,
            "evaluation agent: evaluating self-upgrade"
        );

        // Check that pre-load validation passed
        let preload_passed = ctx.metadata["validation"]["all_passed"]
            .as_bool()
            .unwrap_or(false);

        if !preload_passed {
            return Ok(json!({
                "build_type": "self_upgrade",
                "component": component,
                "new_version": new_version,
                "overall_score": 0.0,
                "recommendation": "discard",
                "reasoning": "Pre-load validation did not pass. Cannot approve upgrade.",
                "artifact_id": ctx.artifact_id,
            }));
        }

        let eval_result = self_upgrade::evaluate_upgrade(component, new_version).await?;

        let overall_score = eval_result["overall_score"].as_f64().unwrap_or(0.0);
        let recommendation = eval_result["recommendation"]
            .as_str()
            .unwrap_or("hold")
            .to_string();

        info!(
            component,
            new_version,
            overall_score = %overall_score,
            recommendation = %recommendation,
            "self-upgrade evaluation complete"
        );

        Ok(json!({
            "build_type": "self_upgrade",
            "evaluation": eval_result,
            "artifact_id": ctx.artifact_id,
            "overall_score": overall_score,
            "recommendation": recommendation,
        }))
    }
}
