use anyhow::{Context, Result};
use serde_json::json;
use tracing::{debug, warn};

use crate::types::{InterpretedVoiceCommand, OrchestratorLlmConfig, VoiceContext};

pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Lightweight LLM client for voice interpretation and event summarisation.
pub struct OrchestratorLlm {
    pub(crate) config: OrchestratorLlmConfig,
    pub(crate) client: reqwest::Client,
}

impl OrchestratorLlm {
    pub fn new(config: OrchestratorLlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Interpret a voice transcript into a structured command.
    ///
    /// If the LLM is disabled or returns an unparseable response, falls back to
    /// a simple keyword-based interpretation.
    pub async fn interpret_voice(
        &self,
        transcript: &str,
        context: &VoiceContext,
    ) -> Result<InterpretedVoiceCommand> {
        if !self.config.enabled {
            return Ok(self.keyword_fallback(transcript, context));
        }

        let api_key = match &self.config.api_key {
            Some(k) => k.clone(),
            None => return Ok(self.keyword_fallback(transcript, context)),
        };

        let model = self
            .config
            .model
            .as_deref()
            .unwrap_or("meta-llama/llama-3.1-8b-instruct");

        let task_list = context
            .active_tasks
            .iter()
            .map(|t| format!("- {} (id={}, state={:?})", t.name, t.id, t.state))
            .collect::<Vec<_>>()
            .join("\n");

        let profile_list = context.available_profiles.join(", ");

        let system_prompt = format!(
            r#"You are an assistant that parses voice commands for a code task manager.

Active tasks:
{task_list}

Available profiles: {profile_list}

Parse the user's transcript into a JSON object with an "action" field and appropriate parameters.
Possible actions:
- new_task: {{action: "new_task", profile: "rust"|null, prompt: "..."}}
- send_message: {{action: "send_message", task_hint: "...", message: "..."}}
- run_command: {{action: "run_command", command: "status"|"cost"|"hibernate"}}
- stop_task: {{action: "stop_task", task_hint: "..."}}
- hibernate_task: {{action: "hibernate_task", task_hint: "..."}}
- passthrough: {{action: "passthrough", text: "..."}}

When in doubt, use passthrough. Respond ONLY with the JSON object, no other text."#
        );

        let url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or(OPENROUTER_URL);

        let body = json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": transcript}
            ],
            "max_tokens": 256,
            "temperature": 0.1,
        });

        let response = self
            .client
            .post(url)
            .bearer_auth(&api_key)
            .json(&body)
            .send()
            .await
            .context("calling orchestrator LLM API")?;

        if !response.status().is_success() {
            warn!(
                "orchestrator-llm: API returned {}, falling back to keyword",
                response.status()
            );
            return Ok(self.keyword_fallback(transcript, context));
        }

        let resp_json: serde_json::Value = response.json().await.context("parsing LLM response")?;

        let content = resp_json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        debug!("orchestrator-llm: raw response: {content}");

        // Strip markdown code fences if present.
        let json_str = content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        match serde_json::from_str::<InterpretedVoiceCommand>(json_str) {
            Ok(cmd) => Ok(cmd),
            Err(e) => {
                warn!("orchestrator-llm: failed to parse response ({e}), falling back");
                Ok(self.keyword_fallback(transcript, context))
            }
        }
    }

    /// Simple keyword-based fallback when LLM is disabled or unavailable.
    fn keyword_fallback(&self, transcript: &str, _context: &VoiceContext) -> InterpretedVoiceCommand {
        let lower = transcript.to_lowercase();

        if lower.contains("new") || lower.contains("start") || lower.contains("create") {
            return InterpretedVoiceCommand::NewTask {
                profile: None,
                prompt: transcript.to_string(),
            };
        }

        if lower.contains("stop") || lower.contains("kill") || lower.contains("cancel") {
            return InterpretedVoiceCommand::StopTask {
                task_hint: "current".to_string(),
            };
        }

        if lower.contains("status") || lower.contains("what") {
            return InterpretedVoiceCommand::RunCommand {
                command: "status".to_string(),
            };
        }

        if lower.contains("hibernate") || lower.contains("sleep") || lower.contains("pause") {
            return InterpretedVoiceCommand::HibernateTask {
                task_hint: "current".to_string(),
            };
        }

        InterpretedVoiceCommand::Passthrough {
            text: transcript.to_string(),
        }
    }
}
