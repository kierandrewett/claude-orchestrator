# Claude Orchestrator

Run Claude Code sessions from anywhere. A Rust server + React dashboard hosted on your homelab, with a lightweight daemon on your main PC that spawns and manages Claude instances. Sessions stream in real time, persist across restarts, and resume automatically.

```
┌─────────────────────────────────────────────────────────┐
│  Browser / Phone                                        │
│  dashboard  ──── WebSocket ────▶  Server (homelab)      │
│                                       │                 │
│                                   WebSocket             │
│                                       │                 │
│                                  Client daemon          │
│                                  (home PC, tray icon)   │
│                                       │                 │
│                                  claude --print ...     │
└─────────────────────────────────────────────────────────┘
```

## Components

| Component | Where it runs | What it does |
|-----------|--------------|--------------|
| **Server** | Homelab (Docker) | WebSocket broker, session state, ntfy notifications, serves dashboard |
| **Dashboard** | Browser (served by server) | Real-time session view, streaming output with syntax highlighting, slash command autocomplete, voice input |
| **Client daemon** | Home PC (systemd) | Spawns Claude processes, streams NDJSON events to server, system tray icon with session badge |

## Homelab setup

### 1. Configure environment

```bash
cp .env.server.example .env
```

Edit `.env`:

```env
CLIENT_TOKEN=your-secret-token        # shared with the client daemon
DASHBOARD_TOKEN=your-secret-token     # for browser access
NTFY_TOKEN=your-ntfy-token            # optional, for phone notifications
PUBLIC_URL=http://homelab.local:8080  # used in ntfy click-through links
```

### 2. Start the server

```bash
docker compose up -d
```

The dashboard is served at `http://homelab.local:8080?token=<DASHBOARD_TOKEN>`.

Session data is persisted to a Docker volume (`claude_data`) and survives restarts.

### 3. Update

```bash
docker compose pull   # if using a registry
# or
docker compose build --no-cache
docker compose up -d
```

## Client daemon setup (home PC)

The client daemon runs as a systemd user service, shows a tray icon with a live session count badge, and opens the dashboard when you click it.

### 1. Install

```bash
cd client
chmod +x install.sh
./install.sh
```

The script:
- Builds the binary (`cargo build --release -p claude-client`)
- Installs it to `~/.local/bin/claude-client`
- Creates `~/.config/claude-client/env` from the template (if not already present)
- Stops and disables any existing service, then re-enables the updated one

### 2. Configure

Edit `~/.config/claude-client/env`:

```env
SERVER_URL=ws://homelab.local:8080/ws/client
CLIENT_TOKEN=your-secret-token        # must match server
DASHBOARD_URL=http://homelab.local:8080?token=your-dashboard-token
DEFAULT_CWD=/home/kieran              # default working directory for Claude
CLAUDE_PATH=claude                    # path to the claude binary
```

### 3. Start

```bash
systemctl --user start claude-client
systemctl --user status claude-client
```

The tray icon appears in your system tray. It shows the Claude logo with a badge indicating active session count. Right-click for the menu.

## Sessions

Sessions are created from the dashboard. Type your prompt — you can mention a file path or directory and Claude will work there. Sessions persist across server restarts and resume automatically when the client reconnects.

### Slash commands

Type `/` in the input bar to see available Claude slash commands with autocomplete. Commands are discovered live from your installed Claude version.

### Voice input

Click the microphone button in the input bar to dictate. Uses the Web Speech API (Chrome/Edge).

### ntfy notifications

Each session sends a single persistent notification to your phone that updates in place as Claude works through phases (thinking → reading → writing → running). On completion it shows duration, token counts, and cost.

## Environment variables

### Server

| Variable | Default | Description |
|----------|---------|-------------|
| `CLIENT_TOKEN` | — | **Required.** Auth token for client daemon connections |
| `DASHBOARD_TOKEN` | — | **Required.** Auth token for dashboard access |
| `HOST` | `0.0.0.0` | Bind address |
| `PORT` | `8080` | Bind port |
| `PUBLIC_URL` | `http://localhost:8080` | Public URL used in ntfy click links |
| `NTFY_URL` | `https://ntfy.sh/claude` | ntfy endpoint |
| `NTFY_TOKEN` | — | ntfy Bearer token |
| `MAX_BUFFER` | `5000` | Max events buffered per session for replay |
| `DATA_DIR` | `./data` | Session persistence directory |

### Client daemon

| Variable | Default | Description |
|----------|---------|-------------|
| `SERVER_URL` | — | **Required.** WebSocket URL of the server (`ws://` or `wss://`) |
| `CLIENT_TOKEN` | — | **Required.** Must match server `CLIENT_TOKEN` |
| `DASHBOARD_URL` | `http://localhost:8080` | URL opened when clicking "Open Dashboard" in tray |
| `DEFAULT_CWD` | `$HOME` | Working directory Claude sessions start in |
| `CLAUDE_PATH` | `claude` | Path to the `claude` binary |
| `RUST_LOG` | `info` (tty) / `warn` (daemon) | Log level |
