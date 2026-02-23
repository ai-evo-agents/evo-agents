#![allow(dead_code)]

use serde_json::{Value, json};
use std::time::Instant;
use tracing::info;

// ─── Health check ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct EndpointHealth {
    pub url: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
}

/// Probe a list of URLs and return health results.
pub async fn check_endpoints(client: &reqwest::Client, urls: &[String]) -> Vec<EndpointHealth> {
    let mut results = Vec::with_capacity(urls.len());

    for url in urls {
        let health = probe_url(client, url).await;
        info!(
            url = %url,
            reachable = health.reachable,
            latency_ms = ?health.latency_ms,
            "endpoint health check"
        );
        results.push(health);
    }

    results
}

async fn probe_url(client: &reqwest::Client, url: &str) -> EndpointHealth {
    let start = Instant::now();

    match client
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => EndpointHealth {
            url: url.to_string(),
            reachable: true,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            status_code: Some(resp.status().as_u16()),
        },
        Err(_) => EndpointHealth {
            url: url.to_string(),
            reachable: false,
            latency_ms: None,
            status_code: None,
        },
    }
}

/// Convert health results into a JSON payload for `agent:health` event.
pub fn health_to_json(agent_id: &str, results: &[EndpointHealth]) -> Value {
    let checks: Vec<Value> = results
        .iter()
        .map(|h| {
            json!({
                "url":         h.url,
                "reachable":   h.reachable,
                "latency_ms":  h.latency_ms,
                "status_code": h.status_code,
            })
        })
        .collect();

    json!({
        "agent_id": agent_id,
        "health_checks": checks,
    })
}
