# Claude Code Orchestrator — Architecture

A Rust system that proxies Claude Code instances running inside VMs/containers, managed through messaging platforms. Each task runs in its own isolated environment with customisable tooling, communicating via Claude Code's NDJSON streaming protocol. An event-driven core emits and consumes events through a bus, with pluggable backends (Telegram, Discord, web) subscribing independently. Voice input (voice messages, calls) is handled natively by each backend using a shared orchestrator LLM in the core for interpretation and summarisation.

---

## Repository Structure

```
claude-orchestrator/
├── Cargo.toml                          # Workspace root
├── Cargo.lock
├── README.md
├── docker-compose.yml
│
├── crates/
│   │
│   │  ── Binaries ──────────────────────────────────────
│   │
│   ├── server/                         # Central daemon — event bus, orchestrator core
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                 # Startup, config loading, backend registration
│   │       ├── orchestrator.rs         # Main event loop (consume BackendEvents, emit OrchestratorEvents)
│   │       ├── task_manager.rs         # Task registry, stdin queue, lifecycle
│   │       ├── commands.rs             # Command parsing and execution
│   │       ├── persistence.rs          # State save/load (debounced JSON)
│   │       ├── idle_watchdog.rs        # 12h hibernation timer
│   │       └── config.rs              # Config file types and loading
│   │
│   ├── client/                         # Runs on your PC — manages VMs, connects to server
│   │   ├── Cargo.toml
│   │   ├── claude-client.service
│   │   ├── install.sh
│   │   └── src/
│   │       ├── main.rs
│   │       ├── connection.rs
│   │       ├── protocol.rs
│   │       ├── session_runner.rs
│   │       ├── history_importer.rs
│   │       └── tray.rs
│   │
│   ├── vm-agent/                       # Runs inside the VM
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   │  ── Core Libraries ────────────────────────────────
│   │
│   ├── shared/                         # Protocol types shared between client & server
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── ndjson/                         # Claude Code NDJSON protocol
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs               # ClaudeEvent, UserInput, all inner types
│   │       ├── transport.rs           # NdjsonTransport (stdin/stdout wrapper)
│   │       ├── coalescer.rs           # CoalescedStream (text buffering)
│   │       └── usage.rs              # UsageStats accumulator
│   │
│   ├── events/                         # Event bus + event types
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── bus.rs                 # EventBus (broadcast + mpsc channels)
│   │       ├── orchestrator_events.rs # OrchestratorEvent enum
│   │       ├── backend_events.rs      # BackendEvent enum
│   │       ├── types.rs              # MessageRef, BackendSource, TaskSummary, SessionPhase
│   │       └── commands.rs           # ParsedCommand enum + parser
│   │
│   ├── containers/                     # Docker container lifecycle
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manager.rs            # ContainerManager (spawn, hibernate, wake, destroy)
│   │       ├── handle.rs             # ContainerHandle (owns NdjsonTransport + metadata)
│   │       ├── config.rs             # ContainerConfig, MountPoint, SessionData
│   │       └── profiles.rs           # Profile loading from TOML
│   │
│   ├── orchestrator-llm/              # Lightweight LLM for voice interpretation + summarisation
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── interpreter.rs        # Voice transcript → structured command
│   │       ├── summariser.rs         # OrchestratorEvents → speakable summary
│   │       └── types.rs             # InterpretedVoiceCommand, VoiceContext
│   │
│   │  ── Messaging Backends ────────────────────────────
│   │
│   ├── backend-traits/                 # The MessagingBackend trait
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── backend-telegram/               # Telegram implementation
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── backend.rs            # MessagingBackend::run()
│   │       ├── topics.rs             # Forum topic management
│   │       ├── reactions.rs          # ReactionTracker (real-time emoji)
│   │       ├── streaming.rs          # Text coalescing + edit-in-place
│   │       ├── files.rs             # File upload/download
│   │       ├── voice.rs             # Voice message handling (STT → orchestrator LLM)
│   │       └── formatting.rs        # Tool summaries, error messages, status
│   │
│   ├── backend-discord/                # Discord implementation (stub)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   │
│   ├── backend-web/                    # Web dashboard backend
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── backend.rs
│   │       └── api.rs
│   │
│   └── backend-stdio/                  # Terminal backend for testing
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
│
├── dashboard/                          # React + TypeScript web UI
│   ├── package.json
│   ├── index.html
│   ├── vite.config.ts
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       ├── router.tsx
│       ├── index.css
│       ├── types.ts
│       ├── api/
│       │   └── client.ts
│       ├── components/
│       │   ├── layout/
│       │   │   ├── Header.tsx
│       │   │   └── Sidebar.tsx
│       │   ├── sessions/
│       │   └── viewer/
│       │       ├── SessionViewer.tsx
│       │       ├── EventStream.tsx
│       │       ├── EventRow.tsx
│       │       ├── InputBar.tsx
│       │       ├── CodeBlock.tsx
│       │       └── StatsPanel.tsx
│       ├── hooks/
│       │   ├── useSSE.ts
│       │   ├── useWebSocket.ts
│       │   └── useLiveDuration.ts
│       ├── lib/
│       │   ├── utils.ts
│       │   └── ws.ts
│       └── store/
│           └── sessions.ts
│
├── docker/
│   ├── Dockerfile.base
│   ├── Dockerfile.node
│   ├── Dockerfile.rust
│   ├── Dockerfile.python
│   └── profiles/
│       ├── base.toml
│       ├── web.toml
│       ├── rust.toml
│       └── python.toml
│
├── config/
│   └── orchestrator.example.toml
│
└── scripts/
    ├── build-images.sh
    └── setup.sh
```

### Dependency Graph

```
server
├── events
├── ndjson
├── containers
├── shared
├── orchestrator-llm
├── backend-telegram
├── backend-discord
├── backend-web
└── backend-stdio

backend-telegram
├── events
├── backend-traits
└── orchestrator-llm        ← for voice message handling

backend-discord
├── events
├── backend-traits
└── orchestrator-llm        ← for voice channel handling

backend-web
├── events
└── backend-traits

backend-stdio
├── events
└── backend-traits

containers
└── ndjson

orchestrator-llm
└── events                  ← needs OrchestratorEvent for summarisation

client
├── shared
└── ndjson
```

### Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/server",
    "crates/client",
    "crates/vm-agent",
    "crates/shared",
    "crates/ndjson",
    "crates/events",
    "crates/containers",
    "crates/orchestrator-llm",
    "crates/backend-traits",
    "crates/backend-telegram",
    "crates/backend-discord",
    "crates/backend-web",
    "crates/backend-stdio",
]

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }
async-trait = "0.1"
reqwest = { version = "0.12", features = ["json"] }
```

---

## Event-Driven Messaging Architecture

### Event Bus

```rust
pub struct EventBus {
    orch_tx: broadcast::Sender<OrchestratorEvent>,
    backend_tx: mpsc::Sender<BackendEvent>,
    backend_rx: mpsc::Receiver<BackendEvent>,
}
```

Orchestrator emits `OrchestratorEvent`s via broadcast (all backends see all events). Backends emit `BackendEvent`s via mpsc (all feed into one channel the orchestrator consumes).

### Backend Trait

```rust
#[async_trait]
pub trait MessagingBackend: Send + Sync + 'static {
    fn name(&self) -> &str;
    async fn run(
        &self,
        orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> Result<()>;
}
```

### OrchestratorEvent (core → backends)

```rust
pub enum OrchestratorEvent {
    PhaseChanged { task_id: TaskId, phase: SessionPhase, trigger_message: Option<MessageRef> },
    TextOutput { task_id: TaskId, text: String, is_continuation: bool },
    ToolStarted { task_id: TaskId, tool_name: String, summary: String },
    ToolCompleted { task_id: TaskId, tool_name: String, summary: String, is_error: bool, output_preview: Option<String> },
    Thinking { task_id: TaskId, text: String },
    TurnComplete { task_id: TaskId, usage: UsageStats, duration_secs: f64 },
    TaskCreated { task_id: TaskId, name: String, profile: String, kind: TaskKind },
    TaskStateChanged { task_id: TaskId, old_state: TaskStateSummary, new_state: TaskStateSummary },
    Error { task_id: Option<TaskId>, error: String, next_steps: Vec<String> },
    QueuedMessageDelivered { task_id: TaskId, original_ref: MessageRef },
    FileOutput { task_id: TaskId, filename: String, data: Arc<Vec<u8>>, mime_type: Option<String>, caption: Option<String> },
    CommandResponse { task_id: Option<TaskId>, text: String },
}
```

### BackendEvent (backends → core)

```rust
pub enum BackendEvent {
    UserMessage { task_id: TaskId, text: String, message_ref: MessageRef, source: BackendSource },
    Command { command: ParsedCommand, task_id: Option<TaskId>, message_ref: MessageRef, source: BackendSource },
    FileUpload { task_id: TaskId, filename: String, data: Arc<Vec<u8>>, mime_type: Option<String>, caption: Option<String>, message_ref: MessageRef, source: BackendSource },
}
```

Notice: no `VoiceCommand` variant. Voice is just a `UserMessage` or `Command` by the time it reaches the bus — the backend already used the orchestrator LLM to interpret it.

---

## Voice Input — Per-Backend Feature

Voice isn't a separate backend. It's a capability that each messaging backend handles natively using its platform's voice features, with a shared orchestrator LLM for interpretation.

### How It Works

```
┌─────────────────────────────────────────────┐
│            Telegram Backend                  │
│                                              │
│  User sends voice message in topic           │
│       │                                      │
│       ▼                                      │
│  Telegram API: getFile → download .ogg       │
│       │                                      │
│       ▼                                      │
│  STT (Whisper via OpenRouter, or             │
│       Telegram's own transcription)          │
│       │                                      │
│       ▼ transcript: "start a rust task       │
│         to fix the parser"                   │
│       │                                      │
│       ▼                                      │
│  OrchestratorLlm::interpret_voice()          │
│       │                                      │
│       ▼ InterpretedVoiceCommand::NewTask     │
│         { profile: "rust",                   │
│           prompt: "fix the parser" }         │
│       │                                      │
│       ▼                                      │
│  Emit BackendEvent::Command or               │
│       BackendEvent::UserMessage              │
│  (voice input is now indistinguishable       │
│   from typed input on the event bus)         │
│                                              │
└──────────────────────────────────────────────┘
```

```
┌─────────────────────────────────────────────┐
│            Discord Backend                   │
│                                              │
│  User speaks in voice channel                │
│       │                                      │
│       ▼                                      │
│  Discord voice receive → PCM audio           │
│       │                                      │
│       ▼                                      │
│  STT (Whisper)                               │
│       │                                      │
│       ▼                                      │
│  OrchestratorLlm::interpret_voice()          │
│       │                                      │
│       ▼                                      │
│  Emit BackendEvent (same as above)           │
│                                              │
│  ── Response path ──                         │
│                                              │
│  OrchestratorEvents arrive                   │
│       │                                      │
│       ▼                                      │
│  OrchestratorLlm::summarise_events()         │
│       │                                      │
│       ▼ "Claude read the file and is         │
│          writing a fix"                      │
│       │                                      │
│       ▼                                      │
│  TTS → play audio in voice channel           │
│                                              │
└──────────────────────────────────────────────┘
```

Each backend uses voice differently — Telegram receives .ogg voice messages, Discord receives live voice channel audio — but they all feed through the same `OrchestratorLlm` for interpretation.

### When Voice Output Makes Sense

Not every event should be spoken. Backends decide:

- **Telegram**: Voice messages are input-only. Responses go as text in the topic (you're looking at your phone anyway). The orchestrator LLM is only used for interpretation, not summarisation.
- **Discord**: Voice channels are bidirectional. Use the orchestrator LLM to summarise events and TTS them into the voice channel. Text responses also go to the text channel in the thread.
- **Web**: Could support browser microphone input. Responses are always visual.

This is entirely the backend's decision. The core doesn't know or care whether a backend uses voice.

---

## Orchestrator LLM (`crates/orchestrator-llm/`)

A shared library that backends import when they need voice/natural language interpretation. Lives in the core because multiple backends use it, but it's a library — not a running service.

### Capabilities

**1. Interpret voice transcript → structured command:**

```rust
impl OrchestratorLlm {
    pub async fn interpret_voice(
        &self,
        transcript: &str,
        context: &VoiceContext,
    ) -> Result<InterpretedVoiceCommand> { ... }
}

pub struct VoiceContext {
    pub active_tasks: Vec<TaskSummary>,
    pub available_profiles: Vec<String>,
    pub current_task: Option<TaskSummary>,  // if sent from a task's channel
}
```

The LLM sees the active task list so it can resolve "the parser task" → task_id. The system prompt instructs it to output JSON matching `InterpretedVoiceCommand`.

**2. Summarise events → speakable text:**

```rust
impl OrchestratorLlm {
    pub async fn summarise_events(
        &self,
        events: &[OrchestratorEvent],
        task_name: &str,
    ) -> Result<String> { ... }
}
```

Takes a batch of events and produces a 1-2 sentence natural summary. Only used by backends that have voice output (Discord voice channels). Telegram doesn't need this.

### InterpretedVoiceCommand

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action")]
pub enum InterpretedVoiceCommand {
    /// "Start a new rust task to fix the parser"
    NewTask { profile: Option<String>, prompt: String },

    /// "Tell the auth task to update the middleware tests"
    SendMessage { task_hint: String, message: String },

    /// "What's the status?" / "How much has it cost?"
    RunCommand { command: String },

    /// "Stop the parser task"
    StopTask { task_hint: String },

    /// "Hibernate everything"
    HibernateTask { task_hint: String },

    /// Couldn't interpret — just pass through as a message to the current task
    Passthrough { text: String },
}
```

The `Passthrough` variant is important — if the LLM can't figure out the intent, it just passes the transcript through as a regular message to the current task's Claude instance. This means voice always works, even for freeform instructions.

### Configuration

```toml
[orchestrator_llm]
enabled = true
provider = "openrouter"
api_key = "env:OPENROUTER_API_KEY"
model = "meta-llama/llama-3.1-8b-instruct"
```

If disabled, backends that receive voice just do STT and pass the raw transcript as a `UserMessage` — no interpretation, just dictation.

---

## Telegram Backend

### Event → Telegram Mapping

| OrchestratorEvent | Action |
|---|---|
| PhaseChanged | setMessageReaction on trigger message |
| TextOutput (new) | sendMessage |
| TextOutput (continuation) | editMessageText |
| ToolStarted | sendMessage (compact one-liner) |
| ToolCompleted | editMessageText on tool message |
| Thinking | sendMessage with spoiler (if enabled) |
| TurnComplete | sendMessage with cost summary |
| TaskCreated | createForumTopic |
| TaskStateChanged → hibernated | "💤 Hibernated" |
| Error | ❌ reaction + error + next steps |
| FileOutput | sendDocument / sendPhoto |
| CommandResponse | sendMessage |

### Reactions

| Reaction | SessionPhase |
|----------|-------------|
| 👀 | Acknowledged |
| 🏗️ | Starting |
| 🔧 | ToolUse |
| 🤔 | Thinking |
| 💬 | Responding |
| ✅ | Complete |
| ❌ | Error |

Real-time transitions on the user's message via `setMessageReaction`.

### Voice Messages

Telegram natively supports voice messages (.ogg). The backend:

1. Receives voice message via Bot API
2. Downloads the .ogg file
3. Transcribes (Whisper API, or Telegram's built-in transcription if available)
4. Passes transcript to `OrchestratorLlm::interpret_voice()` with context (active tasks, current topic's task)
5. Emits the appropriate `BackendEvent` (Command or UserMessage)

The user sees the voice message in chat, then the bot replies with what it interpreted: "🎤 Heard: *start a rust task to fix the parser*" followed by the normal task creation flow.

### Topics

```
Supergroup (forum topics)
├── 📌 Scratchpad (pinned, never auto-hibernates)
├── Task #1 — "Fix lifetime error"
├── Task #2 — "Refactor auth" (💤)
└── ...
```

### Files

Native Telegram attachments. User sends file → bot writes to container → notifies Claude. Claude produces file → bot sends back as attachment.

### Errors

❌ reaction + message with specific error and actionable next steps.

---

## NDJSON Protocol (`crates/ndjson/`)

```rust
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeEvent {
    #[serde(rename = "system")]    System(SystemInfo),
    #[serde(rename = "assistant")] Assistant(AssistantMessage),
    #[serde(rename = "tool_use")]  ToolUse(ToolUseRequest),
    #[serde(rename = "tool_result")] ToolResult(ToolResultEvent),
    #[serde(rename = "result")]    Result(FinalResult),
}

#[derive(Debug, Serialize)]
pub struct UserInput {
    #[serde(rename = "type")]
    pub kind: String,
    pub message: String,
}
```

`NdjsonTransport`: stdin/stdout wrapper. `CoalescedStream`: buffers text 500ms, tool/result events pass through immediately.

---

## Container Runtime (`crates/containers/`)

```rust
impl ContainerManager {
    async fn spawn(config, initial_prompt, resume_session) -> Result<ContainerHandle>;
    async fn hibernate(container_id) -> Result<SessionData>;
    async fn wake(container_id) -> Result<ContainerHandle>;
    async fn destroy(container_id) -> Result<()>;
    async fn recreate(session_data) -> Result<ContainerHandle>;
}
```

Uses `bollard`. Alpine + Claude Code CLI + `--dangerously-skip-permissions` + `--output-format stream-json`.

### Docker Images

```dockerfile
# docker/Dockerfile.base
FROM alpine:latest
RUN apk add --no-cache bash curl git openssh-client ca-certificates nodejs npm
RUN npm install -g @anthropic-ai/claude-code
RUN adduser -D claude
USER claude
WORKDIR /workspace
ENTRYPOINT ["claude", "--output-format", "stream-json", "--verbose", "--dangerously-skip-permissions"]
```

Extended: `Dockerfile.node`, `Dockerfile.rust`, `Dockerfile.python`.

### Profiles

```toml
# docker/profiles/rust.toml
[image]
name = "orchestrator/claude-code:rust"
```

---

## Authentication

Claude Code inside containers needs your Max subscription credentials. The orchestrator handles this with a one-time interactive login flow built into the CLI, then shares the captured credentials with all containers.

### How It Works

On first run (or when credentials expire), the orchestrator CLI runs `claude login` inside a temporary container, which prints an OAuth URL. You open it in your browser, authenticate with your Claude account, and the OAuth callback writes credentials to `~/.claude/.credentials.json` inside the container. The orchestrator captures that file.

```
$ claude-orchestrator setup

🔑 Claude Code authentication required.
   Starting login flow...

   Open this URL in your browser:
   https://claude.ai/oauth/authorize?client_id=...&redirect_uri=...

   Waiting for authentication...
   ✅ Authenticated as kieran@example.com (Max subscription)
   Credentials saved to ~/.local/share/claude-orchestrator/auth/
```

The credentials file looks like:

```json
{
  "claudeAiOauth": {
    "accessToken": "sk-ant-oat01-...",
    "refreshToken": "sk-ant-ort01-...",
    "expiresAt": 1748658860401,
    "scopes": ["user:inference", "user:profile"]
  }
}
```

The `accessToken` is short-lived (hours). The `refreshToken` is what matters — Claude Code uses it to get fresh access tokens automatically.

### Implementation

```rust
/// Stored credentials from the OAuth flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredentials {
    #[serde(rename = "claudeAiOauth")]
    pub claude_ai_oauth: OAuthTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    pub scopes: Vec<String>,
}

pub struct AuthManager {
    credentials_path: PathBuf,   // ~/.local/share/claude-orchestrator/auth/.credentials.json
    claude_home_path: PathBuf,   // ~/.local/share/claude-orchestrator/auth/ (full ~/.claude/ snapshot)
}

impl AuthManager {
    /// Run the interactive OAuth login flow.
    /// Spins up a temporary container, runs `claude login`,
    /// watches stdout for the OAuth URL and prints it,
    /// waits for the credentials file to appear, captures it.
    pub async fn login(&self, docker: &ContainerManager) -> Result<AuthCredentials> {
        // 1. Create a temporary container with entrypoint overridden to `claude`
        //    (no --output-format, just interactive login)
        // 2. Attach to stdout, watch for the OAuth URL line
        // 3. Print the URL for the user to open
        // 4. Wait for claude to write ~/.claude/.credentials.json
        //    (poll via docker exec, or watch stdout for "Login successful")
        // 5. Copy ~/.claude/ out of the container to self.claude_home_path
        // 6. Parse and return the credentials
        // 7. Destroy the temporary container
    }

    /// Check if we have valid credentials
    pub fn has_credentials(&self) -> bool {
        self.credentials_path.exists()
    }

    /// Load stored credentials
    pub fn load(&self) -> Result<AuthCredentials> {
        let data = std::fs::read_to_string(&self.credentials_path)?;
        Ok(serde_json::from_str(&data)?)
    }

    /// Check if the refresh token is still likely valid.
    /// Claude's refresh tokens last a long time but can be revoked.
    /// We can't know for sure without trying — this just checks the file exists.
    pub fn credentials_look_valid(&self) -> bool {
        self.load()
            .map(|c| !c.claude_ai_oauth.refresh_token.is_empty())
            .unwrap_or(false)
    }
}
```

### How Containers Get Credentials

When `ContainerManager::spawn()` creates a container, it bind-mounts the captured `~/.claude/` directory:

```rust
impl ContainerManager {
    async fn spawn(&self, config: ContainerConfig, ...) -> Result<ContainerHandle> {
        let mut mounts = config.mounts.clone();

        // Mount the captured auth directory so Claude Code finds
        // its credentials and onboarding state
        mounts.push(MountPoint {
            host_path: self.auth_manager.claude_home_path.clone(),
            container_path: PathBuf::from("/home/claude/.claude"),
            read_only: false,  // Claude Code needs to write token refreshes
        });

        // ... create container with mounts ...
    }
}
```

`read_only: false` because Claude Code refreshes the access token in-place when it expires. The refresh token stays the same — only the short-lived access token changes.

### Multiple Containers Writing to the Same Mount

Since all containers mount the same credentials directory, concurrent token refreshes could race. This is fine in practice — they're all refreshing the same refresh token and getting equivalent new access tokens. Last-write-wins is harmless here because any valid access token works.

If this ever becomes an issue, the alternative is to copy the credentials directory per-container on spawn (so each gets its own copy) and periodically sync back.

### Re-authentication

If credentials stop working (token revoked, subscription lapsed), the orchestrator:

1. Detects the error from Claude Code's NDJSON output (auth error events)
2. Emits `OrchestratorEvent::Error` with next steps
3. All backends display: "❌ Authentication failed. Run `claude-orchestrator setup` to re-authenticate."

The `setup` subcommand re-runs the login flow, overwrites the credentials, and all containers pick up the new tokens on their next refresh cycle (or on restart).

### Configuration

```toml
[auth]
# Path to the captured ~/.claude/ directory.
# Created by `claude-orchestrator setup` on first run.
# Contains .credentials.json + onboarding state.
credentials_dir = "~/.local/share/claude-orchestrator/auth"
```

That's it. One path. No tokens or API keys to manage in config.

---

## Session Lifecycle

**Running** → **Hibernated** (12h idle or `/hibernate`): SIGTERM → docker stop.

**Hibernated** → **Running** (user message): docker start → re-attach → `--resume`.

**Dead** → **Running**: recreate from SessionData → resume or fresh.

Idle watchdog every 5 minutes, skips Scratchpad.

---

## Mid-Turn Messaging

Commands execute immediately. Messages queue if Claude is mid-turn, drain at next turn boundary. All queued messages concatenated with `---` separators.

---

## Commands

| Command | Description |
|---------|-------------|
| `/new <profile> <prompt>` | New task → new topic/channel |
| `/stop` | Kill current task |
| `/status` | List all tasks |
| `/cost` / `/cost all` | Usage and cost |
| `/hibernate` | Manual hibernate |
| `/profile list` | List profiles |
| `/config thinking on\|off` | Toggle thinking display |

---

## Configuration

```toml
[server]
state_dir = "~/.local/share/claude-orchestrator"

[docker]
socket = "unix:///var/run/docker.sock"
default_profile = "base"
image_prefix = "orchestrator/claude-code"
idle_timeout_hours = 12

[mounts.default]
host = "~/dev"
container = "/workspace"
read_only = false

[auth]
credentials_dir = "~/.local/share/claude-orchestrator/auth"

[orchestrator_llm]
enabled = true
provider = "openrouter"
api_key = "env:OPENROUTER_API_KEY"
model = "meta-llama/llama-3.1-8b-instruct"

[backends.telegram]
enabled = true
bot_token = "env:TELEGRAM_BOT_TOKEN"
supergroup_id = -100123456789
scratchpad_topic_name = "Scratchpad"
allowed_users = [123456789]
voice_stt = "whisper"           # "whisper" | "telegram" (built-in) | "deepgram"
voice_stt_api_key = "env:OPENAI_API_KEY"  # for whisper

[backends.discord]
enabled = false
bot_token = "env:DISCORD_BOT_TOKEN"
guild_id = 123456789
voice_stt = "whisper"
voice_tts = "elevenlabs"        # discord needs TTS for voice channel responses
voice_tts_api_key = "env:ELEVENLABS_API_KEY"

[backends.web]
enabled = true

[display]
show_thinking = false
stream_coalesce_ms = 500
```

---

## Persistence

```rust
pub struct PersistedTask {
    pub id: TaskId,
    pub name: String,
    pub profile: String,
    pub container_id: Option<String>,
    pub session_data: SessionData,
    pub usage: UsageStats,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub state: PersistedTaskState,
    pub kind: TaskKind,
    pub backend_channels: HashMap<String, String>,
}
```

---

## Implementation Phases

### Phase 1: Repo Restructure
Move existing crates into `crates/`. Create empty scaffolds for new crates. Update workspace Cargo.toml. Verify everything compiles.

### Phase 2: NDJSON Transport (`crates/ndjson/`)
Protocol types, transport, coalescer, usage stats. Tests.

### Phase 3: Event Bus + Backend Trait (`crates/events/`, `crates/backend-traits/`, `crates/backend-stdio/`)
Event types, bus, trait, stdio backend for testing.

### Phase 4: Container Runtime (`crates/containers/`)
ContainerManager via bollard. Dockerfile.base. Smoke test.

### Phase 5: Orchestrator Core (`crates/server/`)
Main loop, task registry, stdin queue, commands, persistence, idle watchdog, scratchpad.

### Phase 6: Telegram Backend (`crates/backend-telegram/`)
Reactions, streaming edits, topics, files, error display.

### Phase 7: Orchestrator LLM (`crates/orchestrator-llm/`)
OpenRouter client, interpret_voice(), summarise_events(). Wire into Telegram backend for voice messages.

### Phase 8: Web Backend (`crates/backend-web/`)
Bridge event bus to existing dashboard.

### Phase 9: Polish
Extended images, profiles, graceful shutdown, Discord stub.
