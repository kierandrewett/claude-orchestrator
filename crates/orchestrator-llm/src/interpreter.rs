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

    pub fn config(&self) -> &OrchestratorLlmConfig {
        &self.config
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

    /// Generate a short topic title from the beginning of a Claude response.
    ///
    /// Falls back to extracting the first meaningful line when the LLM is
    /// disabled or unavailable.
    pub async fn suggest_title(&self, response_preview: &str) -> String {
        let fallback = || {
            let text = response_preview.trim();
            let first_line = text.lines().next().unwrap_or(text);
            let clean: String = first_line.trim_start_matches('#').trim().chars().take(60).collect();
            // Trim to a word boundary if we hit the limit.
            if clean.len() == 60 {
                clean.rsplit_once(' ').map(|(s, _)| s.to_string()).unwrap_or(clean)
            } else {
                clean
            }
        };

        if !self.config.enabled {
            return fallback();
        }
        let api_key = match &self.config.api_key {
            Some(k) => k.clone(),
            None => return fallback(),
        };

        let preview: String = response_preview.chars().take(600).collect();
        let model = self.config.model.as_deref().unwrap_or("meta-llama/llama-3.1-8b-instruct");
        let url = self.config.base_url.as_deref().unwrap_or(OPENROUTER_URL);

        let body = json!({
            "model": model,
            "messages": [
                {
                    "role": "system",
                    "content": "Generate a short title (3-6 words, max 50 characters) for a chat thread based on this assistant message. Reply with ONLY the title — no quotes, no punctuation at the end."
                },
                {"role": "user", "content": preview}
            ],
            "max_tokens": 32,
            "temperature": 0.3,
        });

        let resp = match self.client.post(url).bearer_auth(&api_key).json(&body).send().await {
            Ok(r) => r,
            Err(e) => { warn!("suggest_title: request failed: {e}"); return fallback(); }
        };
        if !resp.status().is_success() {
            warn!("suggest_title: API returned {}", resp.status());
            return fallback();
        }
        let resp_json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => { warn!("suggest_title: parse error: {e}"); return fallback(); }
        };
        let title = resp_json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .trim()
            .to_string();

        if title.is_empty() || title.len() > 128 { fallback() } else { title }
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
