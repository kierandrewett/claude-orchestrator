-- V1: initial schema

CREATE TABLE IF NOT EXISTS tasks (
    task_id         TEXT PRIMARY KEY,
    task_name       TEXT NOT NULL,
    session_id      TEXT,
    session_status  TEXT NOT NULL DEFAULT 'stopped',
    created_at      TEXT NOT NULL,
    last_activity   TEXT,
    metadata        TEXT
);

CREATE TABLE IF NOT EXISTS telegram_state (
    task_id         TEXT PRIMARY KEY REFERENCES tasks(task_id) ON DELETE CASCADE,
    topic_id        INTEGER NOT NULL,
    chat_id         INTEGER NOT NULL,
    message_count   INTEGER NOT NULL DEFAULT 0,
    last_message_id INTEGER,
    metadata        TEXT
);

CREATE TABLE IF NOT EXISTS scheduled_events (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL,
    description          TEXT,
    schedule             TEXT NOT NULL,
    mode                 TEXT NOT NULL,
    action_type          TEXT NOT NULL,
    action_data          TEXT NOT NULL,
    enabled              INTEGER NOT NULL DEFAULT 1,
    created_at           TEXT NOT NULL,
    last_run             TEXT,
    next_run             TEXT,
    origin_task_id       TEXT NOT NULL,
    origin_task_name     TEXT NOT NULL,
    consecutive_failures INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS event_executions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id    TEXT NOT NULL REFERENCES scheduled_events(id) ON DELETE CASCADE,
    timestamp   TEXT NOT NULL,
    status      TEXT NOT NULL,
    detail      TEXT
);

CREATE TRIGGER IF NOT EXISTS cap_execution_log
AFTER INSERT ON event_executions
BEGIN
    DELETE FROM event_executions
    WHERE id IN (
        SELECT id FROM event_executions
        WHERE event_id = NEW.event_id
        ORDER BY timestamp DESC
        LIMIT -1 OFFSET 50
    );
END;

CREATE INDEX IF NOT EXISTS idx_events_enabled ON scheduled_events(enabled);
CREATE INDEX IF NOT EXISTS idx_events_next_run ON scheduled_events(next_run);
CREATE INDEX IF NOT EXISTS idx_executions_event_id ON event_executions(event_id);
CREATE INDEX IF NOT EXISTS idx_telegram_topic ON telegram_state(topic_id);
