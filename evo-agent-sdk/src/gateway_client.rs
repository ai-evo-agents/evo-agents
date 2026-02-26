use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::json;
use tracing::{info, warn};

/// HTTP client for calling evo-gateway's OpenAI-compatible chat completion API.
///
/// All agent LLM interactions go through evo-gateway rather than calling
/// providers directly. The gateway handles provider routing, rate limiting,
/// and key management.
pub struct GatewayClient {
    http_client: reqwest::Client,
    gateway_url: String,
}

impl GatewayClient {
    /// Create a new gateway client.
    ///
    /// `gateway_url` should be the base URL of the evo-gateway instance
    /// (e.g. `http://localhost:8080`).
    pub fn new(gateway_url: &str) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .context("Failed to build HTTP client for gateway")?;

        Ok(Self {
            http_client,
            gateway_url: gateway_url.trim_end_matches('/').to_string(),
        })
    }

    /// Send a chat completion request through the gateway.
    ///
    /// Returns the assistant's reply text.
    pub async fn chat_completion(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.gateway_url);

        let mut body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ]
        });

        if let Some(temp) = temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max) = max_tokens {
            body["max_tokens"] = json!(max);
        }

        info!(
            model = %model,
            url = %url,
            "sending chat completion request to gateway"
        );

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Gateway chat completion request failed")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse gateway response")?;

        if !status.is_success() {
            let error = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            anyhow::bail!("Gateway returned {status}: {error}");
        }

        // Extract the assistant message content from OpenAI-compatible response
        let content = resp_body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if content.is_empty() {
            warn!("gateway returned empty response content");
        }

        Ok(content)
    }

    /// Send a streaming chat completion request through the gateway.
    ///
    /// For each SSE chunk containing delta text, calls `on_chunk(delta, chunk_index)`.
    /// Returns the full accumulated response text when the stream completes.
    ///
    /// The gateway returns SSE format: `data: {"choices":[{"delta":{"content":"..."}}]}\n\n`
    /// terminated by `data: [DONE]\n\n`.
    pub async fn chat_completion_streaming<F>(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
        mut on_chunk: F,
    ) -> Result<String>
    where
        F: FnMut(&str, u32) + Send,
    {
        let url = format!("{}/v1/chat/completions", self.gateway_url);

        let mut body = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "stream": true
        });

        if let Some(temp) = temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max) = max_tokens {
            body["max_tokens"] = json!(max);
        }

        info!(
            model = %model,
            url = %url,
            "sending streaming chat completion request to gateway"
        );

        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Gateway streaming request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Gateway returned {status}: {text}");
        }

        let mut stream = resp.bytes_stream();
        let mut accumulated = String::new();
        let mut chunk_index: u32 = 0;
        let mut line_buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.context("Error reading SSE stream chunk")?;
            let text = String::from_utf8_lossy(&chunk);
            line_buffer.push_str(&text);

            // Process complete lines from the SSE stream
            while let Some(pos) = line_buffer.find('\n') {
                let line = line_buffer[..pos].trim().to_string();
                line_buffer = line_buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if line == "data: [DONE]" {
                    break;
                }

                if let Some(json_str) = line.strip_prefix("data: ")
                    && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
                    && let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str()
                    && !delta.is_empty()
                {
                    accumulated.push_str(delta);
                    on_chunk(delta, chunk_index);
                    chunk_index += 1;
                }
            }
        }

        if accumulated.is_empty() {
            warn!("streaming gateway response produced no content");
        }

        Ok(accumulated)
    }
}
