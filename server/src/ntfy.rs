use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

const MIN_INTERVAL: Duration = Duration::from_secs(2);
const FORCE_UPDATE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Starting,
    Thinking,
    Reading,
    Writing,
    Running,
}

impl Phase {
    fn icon(&self) -> &'static str {
        match self {
            Phase::Starting => "🚀",
            Phase::Thinking => "🧠",
            Phase::Reading => "📂",
            Phase::Writing => "✍️",
            Phase::Running => "⚙️",
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Phase::Starting => "Starting",
            Phase::Thinking => "Thinking",
            Phase::Reading => "Reading",
            Phase::Writing => "Writing",
            Phase::Running => "Running",
        }
    }
}

struct SessionActivity {
    phase: Phase,
    targets: Vec<String>, // accumulated targets for current phase
    last_target: Option<String>,
    last_sent: Instant,
    input_tokens: u64,
    output_tokens: u64,
}

impl SessionActivity {
    fn new() -> Self {
        Self {
            phase: Phase::Starting,
            targets: vec![],
            last_target: None,
            last_sent: Instant::now() - FORCE_UPDATE,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    fn format_body(&self) -> String {
        let icon = self.phase.icon();
        let label = self.phase.label();
        if self.targets.is_empty() {
            return format!("{icon} {label}");
        }
        let unique: Vec<_> = {
            let mut seen = std::collections::HashSet::new();
            self.targets
                .iter()
                .filter(|t| seen.insert(t.as_str()))
                .collect()
        };
        let shown = unique
            .iter()
            .take(2)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let extra = if unique.len() > 2 {
            format!(" (+{} more)", unique.len() - 2)
        } else {
            String::new()
        };
        let target_line = format!("{shown}{extra}");
        let target_line = if target_line.len() > 100 {
            format!("{}…", &target_line[..97])
        } else {
            target_line
        };
        format!("{icon} {label}\n→ {target_line}")
    }

    fn should_update(&self, phase_changed: bool) -> bool {
        if phase_changed {
            return true;
        }
        let elapsed = self.last_sent.elapsed();
        if elapsed >= FORCE_UPDATE {
            return true;
        }
        if elapsed >= MIN_INTERVAL {
            if self.targets.last().map(|s| s.as_str()) != self.last_target.as_deref() {
                return true;
            }
        }
        false
    }
}

pub struct NtfyManager {
    url: String,
    token: Option<String>,
    public_url: String,
    client: reqwest::Client,
    sessions: Mutex<HashMap<String, SessionActivity>>,
}

impl NtfyManager {
    pub fn new(url: String, token: Option<String>, public_url: String) -> Arc<Self> {
        Arc::new(Self {
            url,
            token,
            public_url,
            client: reqwest::Client::new(),
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Called when a session starts.
    pub async fn session_started(&self, session_id: &str, name: Option<&str>, cwd: &str) {
        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(session_id.to_string(), SessionActivity::new());
        }
        let title = format!("Claude — {}", name.unwrap_or(cwd));
        self.send(session_id, &title, "🚀 Starting…", 3).await;
    }

    /// Called for each Claude NDJSON event — detects phase, sends update if warranted.
    pub async fn on_event(&self, session_id: &str, event: &serde_json::Value) {
        let (phase, target) = detect_phase(event);
        let phase = match phase {
            Some(p) => p,
            None => return,
        };

        let body = {
            let mut sessions = self.sessions.lock().await;
            let activity = match sessions.get_mut(session_id) {
                Some(a) => a,
                None => return,
            };

            let phase_changed = activity.phase != phase;
            if phase_changed {
                activity.phase = phase.clone();
                activity.targets.clear();
            }
            if let Some(t) = &target {
                activity.targets.push(t.clone());
                activity.last_target = Some(t.clone());
            }

            // Update token counts from event.
            if let Some(tokens) = event
                .pointer("/message/usage/input_tokens")
                .and_then(|v| v.as_u64())
            {
                activity.input_tokens += tokens;
            }
            if let Some(tokens) = event
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64())
            {
                activity.output_tokens += tokens;
            }

            if !activity.should_update(phase_changed) {
                return;
            }
            activity.last_sent = Instant::now();
            activity.format_body()
        };

        let title = format!(
            "Claude — {}",
            session_id.chars().take(6).collect::<String>()
        );
        self.send(session_id, &title, &body, 3).await;
    }

    /// Called when a session ends.
    pub async fn session_ended(
        &self,
        session_id: &str,
        success: bool,
        stats: &crate::protocol::SessionStats,
    ) {
        {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(session_id);
        }

        let duration_str = match stats.cost_usd {
            Some(cost) => format!(
                "${:.4} · {} in · {} out",
                cost, stats.input_tokens, stats.output_tokens
            ),
            None => format!("{} in · {} out", stats.input_tokens, stats.output_tokens),
        };

        let body = if success {
            format!("✅ Done\n{duration_str}")
        } else {
            format!("❌ Failed\n{duration_str}")
        };

        let title = format!(
            "Claude — {}",
            session_id.chars().take(6).collect::<String>()
        );
        self.send(session_id, &title, &body, 4).await;
    }

    async fn send(&self, session_id: &str, title: &str, body: &str, priority: u8) {
        let token = match &self.token {
            Some(t) => t.clone(),
            None => return,
        };
        let url = self.url.clone();
        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("X-Id", session_id)
            .header("X-Title", title)
            .header("X-Priority", priority.to_string())
            .header("X-Click", &self.public_url)
            .body(body.to_string());

        // Add tags based on body content.
        if body.contains('✅') {
            req = req.header("X-Tags", "white_check_mark");
        } else if body.contains('❌') {
            req = req.header("X-Tags", "x");
        } else if body.contains('🧠') {
            req = req.header("X-Tags", "brain");
        }

        if let Err(e) = req.send().await {
            tracing::warn!("ntfy send failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Detect Claude event → Phase + optional target string
// ---------------------------------------------------------------------------

fn detect_phase(event: &serde_json::Value) -> (Option<Phase>, Option<String>) {
    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

    // Streaming format: content_block_start with tool_use
    if event_type == "content_block_start" {
        if event
            .pointer("/content_block/type")
            .and_then(|t| t.as_str())
            == Some("tool_use")
        {
            let name = event
                .pointer("/content_block/name")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            return tool_to_phase(name, &serde_json::Value::Null);
        }
        if event
            .pointer("/content_block/type")
            .and_then(|t| t.as_str())
            == Some("text")
        {
            return (Some(Phase::Thinking), None);
        }
    }

    // Turn-complete format: assistant message with content blocks
    if event_type == "assistant" {
        if let Some(content) = event.pointer("/message/content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let input = block.get("input").unwrap_or(&serde_json::Value::Null);
                    return tool_to_phase(name, input);
                }
            }
        }
    }

    (None, None)
}

fn tool_to_phase(name: &str, input: &serde_json::Value) -> (Option<Phase>, Option<String>) {
    let (phase, key) = match name {
        "Read" | "Glob" | "Grep" => (Phase::Reading, Some("file_path")),
        "Write" | "Edit" | "MultiEdit" => (Phase::Writing, Some("file_path")),
        "Bash" => (Phase::Running, Some("command")),
        "Agent" => (Phase::Thinking, None),
        "WebFetch" | "WebSearch" => (Phase::Running, Some("url")),
        _ => return (None, None),
    };

    let target = key
        .and_then(|k| input.get(k))
        .and_then(|v| v.as_str())
        .map(|s| {
            // basename for file paths
            s.rsplit(['/', '\\']).next().unwrap_or(s).to_string()
        });

    (Some(phase), target)
}
