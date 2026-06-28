CREATE TABLE media_state (
    path TEXT PRIMARY KEY,
    favorite INTEGER NOT NULL DEFAULT 0,
    resume_position_ms INTEGER,
    last_watched_at INTEGER
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);