//! Scans ~/.claude/projects/**/*.jsonl and imports historical Claude Code
//! sessions into the orchestrator server.
//!
//! Imported file paths are tracked in ~/.config/claude-client/imported.txt so
//! they are only sent once across connections.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tracing::{info, warn};

use claude_shared::HistoricalSession;

use crate::connection::{send_msg, Config};
use crate::protocol::C2S;
use crate::session_runner::WsSink;

/// Maximum events (user + assistant lines) kept per session to avoid sending
/// enormous payloads for very long conversations.
const MAX_EVENTS_PER_SESSION: usize = 1_000;

/// Sessions are sent in small batches to avoid stalling the WebSocket.
const BATCH_SIZE: usize = 5;

pub async fn run(config: Arc<Config>, ws_tx: Arc<TokioMutex<WsSink>>) {
    let claude_projects = match dirs::home_dir() {
        Some(h) => h.join(".claude").join("projects"),
        None => {
            warn!("history_importer: cannot determine home directory, skipping import");
            return;
        }
    };

    if !claude_projects.exists() {
        info!("history_importer: ~/.claude/projects not found, nothing to import");
        return;
    }

    let tracking_path = tracking_file_path(&config);
    let already_imported = load_tracking(&tracking_path);

    // Collect JSONL files not yet imported.
    let jsonl_files = collect_jsonl(&claude_projects);
    let new_files: Vec<PathBuf> = jsonl_files
        .into_iter()
        .filter(|p| !already_imported.contains(p.to_string_lossy().as_ref()))
        .collect();

    if new_files.is_empty() {
        info!("history_importer: no new sessions to import");
        return;
    }

    info!(
        "history_importer: found {} new session file(s) to import",
        new_files.len()
    );

    let mut newly_imported: Vec<String> = Vec::new();

    // Process in batches.
    for chunk in new_files.chunks(BATCH_SIZE) {
        let mut sessions: Vec<HistoricalSession> = Vec::new();

        for path in chunk {
            match parse_jsonl(path, &claude_projects) {
                Some(hist) => sessions.push(hist),
                None => {
                    // Unreadable or empty — track it anyway to skip next time.
                    newly_imported.push(path.to_string_lossy().into_owned());
                }
            }
        }

        if !sessions.is_empty() {
            send_msg(&ws_tx, &C2S::ImportHistory { sessions }).await;
        }

        for path in chunk {
            newly_imported.push(path.to_string_lossy().into_owned());
        }
    }

    // Persist tracking.
    append_tracking(&tracking_path, &newly_imported);
    info!(
        "history_importer: import complete, {} file(s) processed",
        newly_imported.len()
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tracking_file_path(config: &Config) -> PathBuf {
    let _ = config; // config available for future use (custom config dir)
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("claude-client")
        .join("imported.txt")
}

fn load_tracking(path: &Path) -> std::collections::HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect()
}

fn append_tracking(path: &Path, entries: &[String]) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        for e in entries {
            writeln!(f, "{e}").ok();
        }
    }
}

/// Recursively collect all .jsonl files under `base`.
fn collect_jsonl(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_jsonl_inner(base, &mut out);
    out
}

fn collect_jsonl_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_inner(&path, out);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

/// Parse a JSONL file into a `HistoricalSession`.
/// Returns `None` if the file is empty or unreadable.
fn parse_jsonl(path: &Path, projects_base: &Path) -> Option<HistoricalSession> {
    let claude_session_id = path.file_stem()?.to_string_lossy().into_owned();

    // Decode CWD from the parent folder name.
    let project_folder = path.parent()?.file_name()?.to_string_lossy();
    let cwd = decode_project_path(&project_folder);

    let content = std::fs::read_to_string(path).ok()?;
    let _ = projects_base; // reserved for future use

    let mut events: Vec<serde_json::Value> = Vec::new();
    let mut created_at: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut ended_at: Option<chrono::DateTime<chrono::Utc>> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        // Capture timestamps from any line that has one.
        if let Some(ts_str) = val.get("timestamp").and_then(|t| t.as_str()) {
            if let Ok(ts) = ts_str.parse::<chrono::DateTime<chrono::Utc>>() {
                if created_at.is_none() {
                    created_at = Some(ts);
                }
                ended_at = Some(ts);
            }
        }

        // Collect only user and assistant turns — the formats EventStream renders.
        let event_type = val.get("type").and_then(|t| t.as_str());
        if matches!(event_type, Some("user") | Some("assistant"))
            && events.len() < MAX_EVENTS_PER_SESSION {
                events.push(val);
            }
    }

    if events.is_empty() {
        return None;
    }

    Some(HistoricalSession {
        claude_session_id,
        cwd,
        events,
        created_at,
        ended_at,
    })
}

/// Convert a Claude project folder name to a filesystem path.
///
/// Claude encodes paths by replacing each `/` with `-` and prepending a `-`
/// for the root slash.  e.g. `-home-kieran-dev-foo` → `/home/kieran/dev/foo`.
///
/// This is ambiguous when directory names contain hyphens; we do a best-effort
/// decode without trying every permutation.
fn decode_project_path(folder: &str) -> String {
    // Strip the leading `-` that represents the root `/`.
    let without_root = folder.strip_prefix('-').unwrap_or(folder);
    format!("/{}", without_root.replace('-', "/"))
}
