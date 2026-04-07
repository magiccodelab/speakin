//! Text replacement storage and application.
//!
//! Replacement pairs are stored via tauri-plugin-store in `text_replacements.json`.

use crate::storage;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const REPLACEMENTS_FILENAME: &str = "text_replacements.json";

/// A single text replacement pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextReplacement {
    pub from: String,
    pub to: String,
}

/// Container for all replacement pairs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextReplacementsFile {
    pub replacements: Vec<TextReplacement>,
}

/// Load replacement pairs from store. Returns empty list on any error (fail-open).
pub fn load_replacements(app: &AppHandle) -> TextReplacementsFile {
    let has_store_data = app
        .store(REPLACEMENTS_FILENAME)
        .ok()
        .and_then(|s| s.get("data"))
        .is_some();

    if has_store_data {
        return storage::load_store_data(app, REPLACEMENTS_FILENAME, "data");
    }

    // Migration: try reading legacy std::fs file
    let Some(data_dir) = app.path().app_data_dir().ok() else {
        let default = TextReplacementsFile::default();
        let _ = storage::save_store_data(app, REPLACEMENTS_FILENAME, "data", &default);
        return default;
    };
    let path = data_dir.join(REPLACEMENTS_FILENAME);
    let data = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                serde_json::from_str::<TextReplacementsFile>(&content).unwrap_or_default()
            }
            Err(e) => {
                log::warn!("读取旧替换词文件失败，跳过迁移: {}", e);
                TextReplacementsFile::default()
            }
        }
    } else {
        TextReplacementsFile::default()
    };

    // Always persist to store after migration (even if empty) to prevent re-reads
    let _ = storage::save_store_data(app, REPLACEMENTS_FILENAME, "data", &data);
    data
}

/// Save replacement pairs to store.
pub fn save_replacements(app: &AppHandle, data: &TextReplacementsFile) -> Result<(), String> {
    storage::save_store_data(app, REPLACEMENTS_FILENAME, "data", data)
}

/// Apply text replacements. Sorts by `from` length descending to avoid
/// short patterns shadowing longer ones (e.g., "ab" before "abc").
pub fn apply_replacements(text: &str, replacements: &[TextReplacement], ignore_case: bool) -> String {
    let mut sorted: Vec<&TextReplacement> = replacements
        .iter()
        .filter(|r| !r.from.is_empty())
        .collect();
    sorted.sort_by(|a, b| b.from.len().cmp(&a.from.len()));

    let mut result = text.to_string();
    for r in sorted {
        if ignore_case {
            result = replace_ignore_case(&result, &r.from, &r.to);
        } else {
            result = result.replace(&r.from, &r.to);
        }
    }
    result
}

/// Case-insensitive string replacement that preserves surrounding text.
fn replace_ignore_case(text: &str, from: &str, to: &str) -> String {
    let lower_text = text.to_lowercase();
    let lower_from = from.to_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut start = 0;
    while let Some(pos) = lower_text[start..].find(&lower_from) {
        result.push_str(&text[start..start + pos]);
        result.push_str(to);
        start += pos + from.len();
    }
    result.push_str(&text[start..]);
    result
}
