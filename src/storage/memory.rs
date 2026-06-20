//! In-memory [`Store`] for tests. Same surface as `SqliteStore` so
//! service-layer tests can swap one in without disk I/O.
//!
//! The implementation is intentionally dumb — a `Vec<Model>` behind
//! a mutex plus a single `AppSettings` cell. We don't bother
//! emulating every SQL edge case (constraints, NULL handling) since
//! the service layer is what should be enforcing business rules.

use std::sync::Arc;

use chrono::Utc;
use parking_lot::Mutex;

use super::model::{Model, ModelPatch, NewModel};
use super::settings::{AppSettings, SettingsPatch};
use super::Store;
use crate::error::{CoreResult, Error};

pub struct InMemoryStore {
    models: Mutex<Vec<Model>>,
    settings: Mutex<AppSettings>,
}

impl InMemoryStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            models: Mutex::new(Vec::new()),
            settings: Mutex::new(AppSettings::default()),
        })
    }
}

impl Store for InMemoryStore {
    fn list_models(&self) -> CoreResult<Vec<Model>> {
        Ok(self.models.lock().clone())
    }

    fn get_model(&self, internal_id: &str) -> CoreResult<Model> {
        self.models
            .lock()
            .iter()
            .find(|m| m.internal_id == internal_id)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("model {internal_id}")))
    }

    fn insert_model(&self, new: NewModel) -> CoreResult<Model> {
        let now = Utc::now();
        let m = Model {
            internal_id: uuid::Uuid::new_v4().to_string(),
            name: new.name,
            model_id: new.model_id,
            base_url: new.base_url,
            api_key: new.api_key,
            anthropic_url: new.anthropic_url,
            model_type: new.model_type,
            openai_tested: false,
            anthropic_tested: false,
            openai_latency: None,
            anthropic_latency: None,
            max_context_tokens: new.max_context_tokens,
            max_input_tokens: new.max_input_tokens,
            max_output_tokens: new.max_output_tokens,
            created_at: now,
            updated_at: now,
        };
        self.models.lock().push(m.clone());
        Ok(m)
    }

    fn update_model(&self, internal_id: &str, patch: ModelPatch) -> CoreResult<Model> {
        let mut models = self.models.lock();
        let m = models
            .iter_mut()
            .find(|m| m.internal_id == internal_id)
            .ok_or_else(|| Error::not_found(format!("model {internal_id}")))?;

        if let Some(v) = patch.name {
            m.name = v;
        }
        if let Some(v) = patch.model_id {
            m.model_id = Some(v);
        }
        if let Some(v) = patch.base_url {
            m.base_url = v;
        }
        if let Some(v) = patch.api_key {
            m.api_key = v;
        }
        if let Some(v) = patch.anthropic_url {
            m.anthropic_url = v;
        }
        if let Some(v) = patch.model_type {
            m.model_type = v;
        }
        if let Some(v) = patch.max_context_tokens {
            m.max_context_tokens = Some(v);
        }
        if let Some(v) = patch.max_input_tokens {
            m.max_input_tokens = Some(v);
        }
        if let Some(v) = patch.max_output_tokens {
            m.max_output_tokens = Some(v);
        }
        if let Some(v) = patch.openai_tested {
            m.openai_tested = v;
        }
        if let Some(v) = patch.anthropic_tested {
            m.anthropic_tested = v;
        }
        if let Some(v) = patch.openai_latency {
            m.openai_latency = Some(v);
        }
        if let Some(v) = patch.anthropic_latency {
            m.anthropic_latency = Some(v);
        }
        m.updated_at = Utc::now();
        Ok(m.clone())
    }

    fn delete_model(&self, internal_id: &str) -> CoreResult<bool> {
        let mut models = self.models.lock();
        let before = models.len();
        models.retain(|m| m.internal_id != internal_id);
        Ok(models.len() < before)
    }

    fn get_settings(&self) -> CoreResult<AppSettings> {
        Ok(self.settings.lock().clone())
    }

    fn save_settings(&self, patch: SettingsPatch) -> CoreResult<AppSettings> {
        let mut s = self.settings.lock();
        if let Some(v) = patch.locale {
            s.locale = Some(v);
        }
        if let Some(v) = patch.theme_mode {
            s.theme_mode = v;
        }
        if let Some(v) = patch.close_to_tray {
            s.close_to_tray = v;
        }
        if let Some(v) = patch.close_window_behavior_set {
            s.close_window_behavior_set = v;
        }
        Ok(s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_roundtrip() {
        let s = InMemoryStore::new();
        let m = s
            .insert_model(NewModel {
                name: "Test".into(),
                model_id: Some("t-1".into()),
                base_url: "https://x.test/v1".into(),
                api_key: "sk-test".into(),
                anthropic_url: None,
                model_type: Default::default(),
                max_context_tokens: Some(1_000_000),
                max_input_tokens: None,
                max_output_tokens: Some(32_000),
            })
            .unwrap();
        assert!(s.get_model(&m.internal_id).is_ok());

        let patched = s
            .update_model(
                &m.internal_id,
                ModelPatch {
                    max_output_tokens: Some(64_000),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(patched.max_output_tokens, Some(64_000));
        // Other token field untouched.
        assert_eq!(patched.max_context_tokens, Some(1_000_000));

        assert!(s.delete_model(&m.internal_id).unwrap());
        assert!(matches!(
            s.get_model(&m.internal_id),
            Err(Error::NotFound { .. })
        ));
    }
}
