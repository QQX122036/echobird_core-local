-- EchoBird persistent schema. Apply once at store open; idempotent
-- because every CREATE uses IF NOT EXISTS.
--
-- Schema version is tracked in the application code (db::SCHEMA_VERSION)
-- via PRAGMA user_version, not by editing this file. To add a
-- column, write a `migrations/m0002_*.sql` and bump the version.

CREATE TABLE IF NOT EXISTS models (
    internal_id           TEXT PRIMARY KEY,
    name                  TEXT NOT NULL,
    model_id              TEXT,
    base_url              TEXT NOT NULL,
    api_key               TEXT NOT NULL,
    anthropic_url         TEXT,
    model_type            TEXT NOT NULL DEFAULT 'CLOUD',
    openai_tested         INTEGER NOT NULL DEFAULT 0,
    anthropic_tested      INTEGER NOT NULL DEFAULT 0,
    openai_latency        INTEGER,
    anthropic_latency     INTEGER,
    max_context_tokens    INTEGER,
    max_input_tokens      INTEGER,
    max_output_tokens     INTEGER,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_models_name ON models(name);

CREATE TABLE IF NOT EXISTS settings (
    id                            INTEGER PRIMARY KEY CHECK (id = 1),
    locale                        TEXT,
    theme_mode                    TEXT,
    close_to_tray                 INTEGER,
    close_window_behavior_set     INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO settings (id, close_window_behavior_set) VALUES (1, 0);
