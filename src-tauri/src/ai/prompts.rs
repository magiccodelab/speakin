//! Prompt template storage and management.
//!
//! Prompts are stored via tauri-plugin-store in `prompts.json`.

use crate::storage;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const PROMPTS_FILENAME: &str = "prompts.json";

/// A prompt template for AI text optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    pub system_prompt: String,
    pub user_prompt_template: String,
    #[serde(default)]
    pub is_builtin: bool,
}

/// Container for all prompt templates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsFile {
    pub prompts: Vec<PromptTemplate>,
}

impl Default for PromptsFile {
    fn default() -> Self {
        Self {
            prompts: builtin_prompts(),
        }
    }
}

/// Embedded default prompts JSON. Keeps default data out of code.
const DEFAULT_PROMPTS_JSON: &str = include_str!("default_prompts.json");

/// Load builtin prompt templates from the embedded JSON file.
fn builtin_prompts() -> Vec<PromptTemplate> {
    serde_json::from_str(DEFAULT_PROMPTS_JSON)
        .expect("default_prompts.json must be valid PromptTemplate array")
}

/// Load prompts from store. Returns defaults if not found or corrupted.
pub fn load_prompts(app: &AppHandle) -> PromptsFile {
    // Try store first
    let store_has_data = app
        .store(PROMPTS_FILENAME)
        .ok()
        .and_then(|s| s.get("data"))
        .is_some();

    let mut file: PromptsFile = if store_has_data {
        storage::load_store_data(app, PROMPTS_FILENAME, "data")
    } else {
        // Migration: try reading legacy std::fs file
        let Some(data_dir) = app.path().app_data_dir().ok() else {
            let default = PromptsFile::default();
            let _ = storage::save_store_data(app, PROMPTS_FILENAME, "data", &default);
            return default;
        };
        let path = data_dir.join(PROMPTS_FILENAME);
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str::<PromptsFile>(&content).unwrap_or_default(),
                Err(e) => {
                    log::warn!("读取旧提示词文件失败，使用默认值: {}", e);
                    PromptsFile::default()
                }
            }
        } else {
            PromptsFile::default()
        }
    };

    // Ensure builtin prompts always exist, in the defined order, with up-to-date metadata.
    // Stale builtins from previous versions (different IDs) are removed.
    let defaults = builtin_prompts();
    let default_ids: std::collections::HashSet<&str> =
        defaults.iter().map(|p| p.id.as_str()).collect();
    let mut changed = false;
    let before_len = file.prompts.len();
    file.prompts
        .retain(|p| !p.is_builtin || default_ids.contains(p.id.as_str()));
    if file.prompts.len() != before_len {
        changed = true;
    }
    // Re-insert defaults in declared order at the front (preserving relative order
    // of any user-modified copies by overwriting in place).
    for (idx, builtin) in defaults.into_iter().enumerate() {
        if let Some(pos) = file.prompts.iter().position(|p| p.id == builtin.id) {
            let existing = &mut file.prompts[pos];
            if existing.category != builtin.category || existing.name != builtin.name {
                existing.category = builtin.category.clone();
                existing.name = builtin.name.clone();
                changed = true;
            }
            if pos != idx {
                let item = file.prompts.remove(pos);
                file.prompts.insert(idx, item);
                changed = true;
            }
        } else {
            file.prompts.insert(idx, builtin);
            changed = true;
        }
    }

    // Persist to store if migrated or builtins changed
    if !store_has_data || changed {
        let _ = storage::save_store_data(app, PROMPTS_FILENAME, "data", &file);
    }

    file
}

/// Save prompts to store.
pub fn save_prompts(app: &AppHandle, prompts: &PromptsFile) -> Result<(), String> {
    storage::save_store_data(app, PROMPTS_FILENAME, "data", prompts)
}
