use anyhow::{Context, Result};
use serde_json::json;
use tracing::warn;

use claude_events::OrchestratorEvent;

use crate::interpreter::OrchestratorLlm;

impl OrchestratorLlm {
    /// Summarise a batch of orchestrator events into 1-2 spoken sentences.
    ///
    /// Used by voice-output backends (Discord) to describe what Claude is doing.
    pub async fn summarise_events(
        &self,
        events: &[OrchestratorEvent],
        task_name: &str,
    ) -> Result<String> {
        if !self.config.enabled || events.is_empty() {
            return Ok(self.plain_summary(events, task_name));
        }

        let api_key = match &self.config.api_key {
            Some(k) => k.clone(),
            None => return Ok(self.plain_summary(events, task_name)),
        };

        let model = self
            .config
            .model
            .as_deref()
            .unwrap_or("meta-llama/llama-3.1-8b-instruct");

        let event_descriptions = events
            .iter()
            .map(describe_event)
            .collect::<Vec<_>>()
            .join("\n");

        let system = "Summarise the following code task events in 1-2 spoken sentences. \
                      Be concise and natural. Do not include technical jargon. \
                      Focus on what was accomplished or is happening.";

        let user = format!(
            "Task: {task_name}\nEvents:\n{event_descriptions}"
        );

        let url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or(crate::interpreter::OPENROUTER_URL);

        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ],
            "max_tokens": 128,
            "temperature": 0.3,
        });

        let resp = self
            .client
            .post(url)
            .bearer_auth(&api_key)
            .json(&body)
            .send()
            .await
            .context("calling summariser LLM API")?;

        if !resp.status().is_success() {
            warn!("orchestrator-llm: summariser API returned {}", resp.status());
            return Ok(self.plain_summary(events, task_name));
        }

        let resp_json: serde_json::Value = resp.json().await.context("parsing summariser response")?;
        let content = resp_json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if content.is_empty() {
            Ok(self.plain_summary(events, task_name))
        } else {
            Ok(content)
        }
    }

    fn plain_summary(&self, events: &[OrchestratorEvent], task_name: &str) -> String {
        let descriptions: Vec<String> = events.iter().map(describe_event).collect();
        format!("Task {task_name}: {}", descriptions.join(". "))
    }
}

fn describe_event(event: &OrchestratorEvent) -> String {
    match event {
        OrchestratorEvent::TextOutput { text, .. } => {
            let preview: String = text.chars().take(80).collect();
            format!("Claude said: {preview}")
        }
        OrchestratorEvent::ToolStarted { tool_name, summary, .. } => {
            format!("Used tool {tool_name}: {summary}")
        }
        OrchestratorEvent::ToolCompleted { tool_name, is_error, .. } => {
            if *is_error {
                format!("{tool_name} failed")
            } else {
                format!("{tool_name} succeeded")
            }
        }
        OrchestratorEvent::TurnComplete { usage, duration_secs, .. } => {
            format!(
                "Turn complete in {duration_secs:.1}s, cost ${:.4}",
                usage.total_cost_usd
            )
        }
        OrchestratorEvent::PhaseChanged { phase, .. } => {
            format!("Phase changed to {phase:?}")
        }
        OrchestratorEvent::Error { error, .. } => {
            format!("Error: {error}")
        }
        _ => String::new(),
    }
}
