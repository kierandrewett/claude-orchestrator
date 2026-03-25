use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_initial.sql")),
];

pub fn run_migrations(conn: &Connection, data_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    let current_version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for &(version, sql) in MIGRATIONS {
        if current_version < version {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version) VALUES (?1)",
                [version],
            )?;
            tx.commit()?;
            tracing::info!("Applied DB migration v{version}");
        }
    }

    migrate_json_files(conn, data_dir)?;
    Ok(())
}

fn migrate_json_files(conn: &Connection, data_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use super::legacy::{LegacyState, LegacyTelegramState};

    let state_path = data_dir.join("state.json");
    if state_path.exists() {
        let raw = std::fs::read_to_string(&state_path)?;
        if let Ok(state) = serde_json::from_str::<LegacyState>(&raw) {
            let count = state.tasks.len();
            let tx = conn.unchecked_transaction()?;
            for task in &state.tasks {
                tx.execute(
                    "INSERT OR IGNORE INTO tasks
                        (task_id, task_name, session_id, session_status, created_at, last_activity)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        task.task_id,
                        task.task_name,
                        task.session_id,
                        task.session_status.as_deref().unwrap_or("stopped"),
                        task.created_at,
                        task.last_activity,
                    ],
                )?;
            }
            tx.commit()?;
            std::fs::rename(&state_path, data_dir.join("state.json.migrated"))?;
            tracing::info!("Migrated {count} tasks from state.json");
        }
    }

    let telegram_path = data_dir.join("telegram_state.json");
    if telegram_path.exists() {
        let raw = std::fs::read_to_string(&telegram_path)?;
        if let Ok(tg) = serde_json::from_str::<LegacyTelegramState>(&raw) {
            // LegacyTelegramState: scratchpad_thread_id, task_topics: HashMap<task_id, thread_id>
            // We don't have chat_id in the legacy file, so we skip telegram_state migration
            // (it's not critical — the Telegram backend re-syncs on startup anyway)
            tracing::info!("telegram_state.json found — keeping as-is (Telegram backend handles its own state)");
            // Don't rename — Telegram backend still reads it on startup
            let _ = tg; // suppress unused warning
        }
    }

    Ok(())
}
