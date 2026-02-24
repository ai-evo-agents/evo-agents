use evo_agent_sdk::AgentRunner;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    AgentRunner::run_kernel().await
}
