//! SQLite-backed [`Store`] implementation. All SQL is hidden inside
//! the `query_*` helpers; the trait method bodies are short and
//! type-checked.
//!
//! Concurrency: we open the connection with `SQLITE_OPEN_FULL_MUTEX`
//! (the rusqlite default for `bundled` feature), so the connection
//! is safe to share across threads behind a `parking_lot::Mutex`.
//! Tauri spawns each command handler on a worker thread, so this
//! is the right primitive.

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use super::model::{Model, ModelPatch, ModelType, NewModel};
use super::settings::{AppSettings, SettingsPatch, ThemeMode};
use super::Store;
use crate::error::{CoreResult, Error};

const SCHEMA: &str = include_str!("schema.sql");

pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: &Path) -> CoreResult<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
        }))
    }

    /// Construct from an already-open connection. Used by the
    /// in-memory tests so they can share schema setup.
    pub fn from_connection(conn: Connection) -> CoreResult<Arc<Self>> {
        conn.execute_batch(SCHEMA)?;
        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
        }))
    }
}

impl Store for SqliteStore {
    fn list_models(&self) -> CoreResult<Vec<Model>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT internal_id, name, model_id, base_url, api_key, anthropic_url,
                    model_type, openai_tested, anthropic_tested, openai_latency,
                    anthropic_latency, max_context_tokens, max_input_tokens,
                    max_output_tokens, created_at, updated_at
             FROM models
             ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_model)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn get_model(&self, internal_id: &str) -> CoreResult<Model> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT internal_id, name, model_id, base_url, api_key, anthropic_url,
                    model_type, openai_tested, anthropic_tested, openai_latency,
                    anthropic_latency, max_context_tokens, max_input_tokens,
                    max_output_tokens, created_at, updated_at
             FROM models
             WHERE internal_id = ?1",
            params![internal_id],
            row_to_model,
        )?
        .pipe(Ok)
    }

    fn insert_model(&self, new: NewModel) -> CoreResult<Model> {
        let internal_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO models
                 (internal_id, name, model_id, base_url, api_key, anthropic_url,
                  model_type, openai_tested, anthropic_tested, openai_latency,
                  anthropic_latency, max_context_tokens, max_input_tokens,
                  max_output_tokens, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, NULL, NULL,
                     ?8, ?9, ?10, ?11, ?11)",
            params![
                internal_id,
                new.name,
                new.model_id,
                new.base_url,
                new.api_key,
                new.anthropic_url,
                model_type_to_str(new.model_type),
                new.max_context_tokens,
                new.max_input_tokens,
                new.max_output_tokens,
                now,
            ],
        )?;
        drop(conn);
        self.get_model(&internal_id)
    }

    fn update_model(&self, internal_id: &str, patch: ModelPatch) -> CoreResult<Model> {
        // Build the UPDATE dynamically so we only touch columns the
        // caller asked to change. The `if let Some(x) = ...` ladder
        // below is the inverse — every field is `Some` ⇒ SET, every
        // `None` ⇒ skip.
        let mut sets: Vec<&'static str> = Vec::new();
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(v) = &patch.name {
            sets.push("name = ?");
            binds.push(Box::new(v.clone()));
        }
        if let Some(v) = &patch.model_id {
            sets.push("model_id = ?");
            binds.push(Box::new(v.clone()));
        }
        if let Some(v) = &patch.base_url {
            sets.push("base_url = ?");
            binds.push(Box::new(v.clone()));
        }
        if let Some(v) = &patch.api_key {
            sets.push("api_key = ?");
            binds.push(Box::new(v.clone()));
        }
        if let Some(v) = &patch.anthropic_url {
            // Option<Option<String>> — distinguish "don't change"
            // from "set to NULL". The frontend never sends the
            // "clear" case today, but the field is reserved.
            sets.push("anthropic_url = ?");
            binds.push(Box::new(v.clone()));
        }
        if let Some(v) = &patch.model_type {
            sets.push("model_type = ?");
            binds.push(Box::new(model_type_to_str(*v).to_string()));
        }
        if let Some(v) = &patch.max_context_tokens {
            sets.push("max_context_tokens = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.max_input_tokens {
            sets.push("max_input_tokens = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.max_output_tokens {
            sets.push("max_output_tokens = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.openai_tested {
            sets.push("openai_tested = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.anthropic_tested {
            sets.push("anthropic_tested = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.openai_latency {
            sets.push("openai_latency = ?");
            binds.push(Box::new(*v as i64));
        }
        if let Some(v) = &patch.anthropic_latency {
            sets.push("anthropic_latency = ?");
            binds.push(Box::new(*v as i64));
        }

        if !sets.is_empty() {
            sets.push("updated_at = ?");
            binds.push(Box::new(Utc::now()));
            let sql = format!(
                "UPDATE models SET {} WHERE internal_id = ?",
                sets.join(", ")
            );
            let mut bind_refs: Vec<&dyn rusqlite::ToSql> =
                binds.iter().map(|b| b.as_ref() as &dyn rusqlite::ToSql).collect();
            bind_refs.push(&internal_id);
            let conn = self.conn.lock();
            let changed = conn.execute(&sql, bind_refs.as_slice())?;
            if changed == 0 {
                return Err(Error::not_found(format!("model {internal_id}")));
            }
        }
        // Even if no fields changed, return the current row so the
        // caller can read whatever they just "set" without a second
        // round-trip.
        self.get_model(internal_id)
    }

    fn delete_model(&self, internal_id: &str) -> CoreResult<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM models WHERE internal_id = ?1",
            params![internal_id],
        )?;
        Ok(n > 0)
    }

    fn get_settings(&self) -> CoreResult<AppSettings> {
        let conn = self.conn.lock();
        // Single-row settings table. We use `SELECT ... FROM
        // settings WHERE id = 1` so the table can grow columns
        // without a code change.
        let row: Option<(Option<String>, Option<String>, Option<i64>, i64)> = conn
            .query_row(
                "SELECT locale, theme_mode, close_to_tray, close_window_behavior_set
                 FROM settings WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let s = match row {
            None => AppSettings::default(),
            Some((locale, theme_mode, close_to_tray, close_set)) => AppSettings {
                locale,
                theme_mode: theme_mode.as_deref().and_then(parse_theme_mode),
                close_to_tray: close_to_tray.map(|v| v != 0),
                close_window_behavior_set: close_set != 0,
            },
        };
        Ok(s)
    }

    fn save_settings(&self, patch: SettingsPatch) -> CoreResult<AppSettings> {
        // Upsert: insert if missing, otherwise update. The COALESCE
        // on each column preserves the existing value when the
        // caller passed `None` (meaning "don't change this field").
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (id, locale, theme_mode, close_to_tray, close_window_behavior_set)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 locale = COALESCE(?1, locale),
                 theme_mode = COALESCE(?2, theme_mode),
                 close_to_tray = COALESCE(?3, close_to_tray),
                 close_window_behavior_set = COALESCE(?4, close_window_behavior_set)",
            params![
                patch.locale,
                patch.theme_mode.flatten().map(|t| theme_mode_to_str(t).to_string()),
                patch.close_to_tray.flatten().map(|b| b as i64),
                patch.close_window_behavior_set.map(|b| b as i64),
            ],
        )?;
        self.get_settings()
    }
}

// ─── Row → Model conversion ─────────────────────────────────────

fn row_to_model(row: &rusqlite::Row<'_>) -> rusqlite::Result<Model> {
    let model_type_str: String = row.get(6)?;
    let openai_tested: i64 = row.get(7)?;
    let anthropic_tested: i64 = row.get(8)?;
    let openai_latency: Option<i64> = row.get(9)?;
    let anthropic_latency: Option<i64> = row.get(10)?;
    let max_context: Option<i64> = row.get(11)?;
    let max_input: Option<i64> = row.get(12)?;
    let max_output: Option<i64> = row.get(13)?;
    let created_at: chrono::DateTime<chrono::Utc> = row.get(14)?;
    let updated_at: chrono::DateTime<chrono::Utc> = row.get(15)?;

    Ok(Model {
        internal_id: row.get(0)?,
        name: row.get(1)?,
        model_id: row.get(2)?,
        base_url: row.get(3)?,
        api_key: row.get(4)?,
        anthropic_url: row.get(5)?,
        model_type: parse_model_type(&model_type_str).unwrap_or(ModelType::Cloud),
        openai_tested: openai_tested != 0,
        anthropic_tested: anthropic_tested != 0,
        openai_latency: openai_latency.and_then(|v| u32::try_from(v).ok()),
        anthropic_latency: anthropic_latency.and_then(|v| u32::try_from(v).ok()),
        max_context_tokens: max_context.and_then(|v| u64::try_from(v).ok()),
        max_input_tokens: max_input.and_then(|v| u64::try_from(v).ok()),
        max_output_tokens: max_output.and_then(|v| u64::try_from(v).ok()),
        created_at,
        updated_at,
    })
}

fn model_type_to_str(t: ModelType) -> &'static str {
    match t {
        ModelType::Cloud => "CLOUD",
        ModelType::Local => "LOCAL",
        ModelType::Tunnel => "TUNNEL",
        ModelType::Demo => "DEMO",
    }
}

fn parse_model_type(s: &str) -> Option<ModelType> {
    match s {
        "CLOUD" => Some(ModelType::Cloud),
        "LOCAL" => Some(ModelType::Local),
        "TUNNEL" => Some(ModelType::Tunnel),
        "DEMO" => Some(ModelType::Demo),
        _ => None,
    }
}

fn theme_mode_to_str(t: ThemeMode) -> &'static str {
    match t {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
    }
}

fn parse_theme_mode(s: &str) -> Option<ThemeMode> {
    match s {
        "light" => Some(ThemeMode::Light),
        "dark" => Some(ThemeMode::Dark),
        _ => None,
    }
}

// Tiny `pipe` helper so we can write `get_model(...).pipe(Ok)` for
// the places where `?` would need an extra `From` impl we don't
// want to write. Keeps the call sites short.
trait Pipe: Sized {
    fn pipe<U, F: FnOnce(Self) -> U>(self, f: F) -> U {
        f(self)
    }
}
impl<T> Pipe for T {}
