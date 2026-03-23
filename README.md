# Claude Code Orchestrator

A Rust system that proxies Claude Code instances running inside Docker containers, managed through messaging platforms. Each task runs in its own isolated environment with customisable tooling, communicating via Claude Code's NDJSON streaming protocol.

An event-driven core emits and consumes events through a bus, with pluggable backends (Telegram, web, stdio) subscribing independently. Voice input is handled natively by each backend using a shared orchestrator LLM for interpretation.

## Quick Start

### Prerequisites

- Docker (socket at `/var/run/docker.sock`)
- Rust stable toolchain
- Claude Max subscription

### Setup

```bash
cargo build --release -p claude-server

# Build images + authenticate (interactive)
./target/release/claude-orchestrator setup

# Copy and edit config
cp config/orchestrator.example.toml config/orchestrator.toml
$EDITOR config/orchestrator.toml
```

### Run

```bash
./target/release/claude-orchestrator run --config config/orchestrator.toml
```

## Architecture Overview

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full design document.

```
┌──────────────┐   OrchestratorEvent (broadcast)   ┌──────────────────┐
│  Orchestrator │ ─────────────────────────────────▶│    Backends      │
│  (server)     │                                    │ telegram/web/... │
│               │ ◀─────────────────────────────────│                  │
└──────┬────────┘    BackendEvent (mpsc)             └──────────────────┘
       │ NDJSON (stdin/stdout via bollard)
       ▼
┌──────────────┐
│   Docker     │   one container per task, running Claude Code
│  Containers  │
└──────────────┘
```

## Configuration Reference

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `[server]` | `state_dir` | `~/.local/share/claude-orchestrator` | Persisted state |
| `[docker]` | `socket` | `unix:///var/run/docker.sock` | Docker socket |
| `[docker]` | `default_profile` | `base` | Default container profile |
| `[docker]` | `idle_timeout_hours` | `12` | Hours before auto-hibernation |
| `[auth]` | `credentials_dir` | `~/.local/share/claude-orchestrator/auth` | OAuth credentials |
| `[orchestrator_llm]` | `enabled` | `true` | Enable LLM for voice interpretation |
| `[orchestrator_llm]` | `api_key` | — | Use `"env:VAR_NAME"` for env vars |
| `[backends.telegram]` | `bot_token` | — | From @BotFather |
| `[backends.telegram]` | `supergroup_id` | — | Forum-topics supergroup |
| `[display]` | `show_thinking` | `false` | Show Claude's internal thinking |

See `config/orchestrator.example.toml` for the full example.

## Available Commands

| Command | Description |
|---------|-------------|
| `/new <profile> <prompt>` | Create a new task |
| `/stop [task-id]` | Stop the current task |
| `/status` | List all tasks with state and cost |
| `/cost [all]` | Show cost for current or all tasks |
| `/hibernate` | Hibernate the current task |
| `/profile list` | List available profiles |
| `/config thinking on\|off` | Toggle thinking display |

## Docker Profiles

| Profile | Includes |
|---------|----------|
| `base` | Node.js, npm, Claude Code |
| `web` | + yarn, pnpm |
| `rust` | + rustup, cargo |
| `python` | + python3, pip |

Build all images: `./scripts/build-images.sh`

## Adding a New Messaging Backend

Implement `MessagingBackend` from `crates/backend-traits/`:

```rust
#[async_trait]
impl MessagingBackend for MyBackend {
    fn name(&self) -> &str { "my-backend" }

    async fn run(
        &self,
        mut orchestrator_events: broadcast::Receiver<OrchestratorEvent>,
        backend_sender: mpsc::Sender<BackendEvent>,
    ) -> anyhow::Result<()> {
        // Handle OrchestratorEvents, emit BackendEvents.
    }
}
```

Backends depend only on `claude-events` + `backend-traits`. Register in `crates/server/src/main.rs`.
