//! AI provider instance storage and management.
//!
//! Each AI provider instance has its own URL, model, stream toggle, and extra_body.
//! API keys are stored per-instance in OS keyring as `ai_provider_<id>`.
//! Provider data is stored via tauri-plugin-store in `ai_providers.json`.

use crate::storage;
use keyring::error::Error as KeyringError;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const PROVIDERS_FILENAME: &str = "ai_providers.json";

/// API protocol type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ApiProtocol {
    Openai,
    Gemini,
}

impl Default for ApiProtocol {
    fn default() -> Self {
        Self::Openai
    }
}

/// A single AI provider instance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub protocol: ApiProtocol,
    pub api_endpoint: String,
    pub model: String,
    #[serde(default = "default_true")]
    pub stream: bool,
    /// Extra JSON fields merged into the request body (top-level).
    /// Reserved keys are skipped during merge (protocol-specific).
    #[serde(default = "default_extra_body")]
    pub extra_body: serde_json::Value,
}

fn default_true() -> bool {
    true
}

fn default_extra_body() -> serde_json::Value {
    serde_json::json!({})
}

/// Container for all AI provider instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvidersFile {
    pub providers: Vec<AiProvider>,
}

impl Default for AiProvidersFile {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
        }
    }
}

/// Load providers from store. Returns empty list if not found or corrupted.
pub fn load_providers(app: &AppHandle) -> AiProvidersFile {
    let has_store_data = app
        .store(PROVIDERS_FILENAME)
        .ok()
        .and_then(|s| s.get("data"))
        .is_some();

    if has_store_data {
        return storage::load_store_data(app, PROVIDERS_FILENAME, "data");
    }

    // Migration: try reading legacy std::fs file
    let Some(data_dir) = app.path().app_data_dir().ok() else {
        let default = AiProvidersFile::default();
        let _ = storage::save_store_data(app, PROVIDERS_FILENAME, "data", &default);
        return default;
    };
    let path = data_dir.join(PROVIDERS_FILENAME);
    let data = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str::<AiProvidersFile>(&content).unwrap_or_default(),
            Err(e) => {
                log::warn!("读取旧 AI 供应商文件失败，跳过迁移: {}", e);
                AiProvidersFile::default()
            }
        }
    } else {
        AiProvidersFile::default()
    };

    // Always persist to store after migration (even if empty) to prevent re-reads
    let _ = storage::save_store_data(app, PROVIDERS_FILENAME, "data", &data);
    data
}

/// Save providers to store.
pub fn save_providers(app: &AppHandle, data: &AiProvidersFile) -> Result<(), String> {
    storage::save_store_data(app, PROVIDERS_FILENAME, "data", data)
}

/// Keyring key for a specific provider's API key.
fn keyring_key(provider_id: &str) -> String {
    format!("ai_provider_{}", provider_id)
}

// ── Per-provider keyring operations ──

/// Check if a provider's API key exists in keyring.
pub fn has_provider_key(provider_id: &str) -> Result<bool, String> {
    let key = keyring_key(provider_id);
    match storage::load_credential(&key) {
        Ok(_) => Ok(true),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(e) => Err(format!("无法访问系统密钥链: {}", e)),
    }
}

/// Load a provider's API key from keyring.
pub fn load_provider_key(provider_id: &str) -> Option<String> {
    let key = keyring_key(provider_id);
    match storage::load_credential(&key) {
        Ok(v) => Some(v),
        _ => None,
    }
}

/// Save a provider's API key to keyring.
pub fn save_provider_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    let key = keyring_key(provider_id);
    storage::save_credential(&key, api_key)
}

/// Clear a provider's API key from keyring.
pub fn clear_provider_key(provider_id: &str) -> Result<(), String> {
    let key = keyring_key(provider_id);
    storage::save_credential(&key, "")
}

// ── Validation ──

/// Validate a provider's fields before save.
pub fn validate_provider(provider: &AiProvider) -> Result<(), String> {
    if provider.name.trim().is_empty() {
        return Err("供应商名称不能为空".to_string());
    }
    if provider.model.trim().is_empty() {
        return Err("模型名称不能为空".to_string());
    }
    if provider.api_endpoint.trim().is_empty() {
        return Err("API 端点不能为空".to_string());
    }
    // Scheme validation: only allow http/https
    let ep_lower = provider.api_endpoint.trim().to_lowercase();
    if !ep_lower.starts_with("http://") && !ep_lower.starts_with("https://") {
        return Err("API 端点必须以 http:// 或 https:// 开头".to_string());
    }
    // Protocol-specific endpoint validation
    match provider.protocol {
        ApiProtocol::Openai => {
            if provider.api_endpoint.ends_with("/chat/completions") {
                return Err(
                    "API 端点不需要包含 /chat/completions 路径（将自动拼接）".to_string(),
                );
            }
        }
        ApiProtocol::Gemini => {
            let ep = &provider.api_endpoint;
            if ep.contains("/models/") {
                return Err("API 端点不需要包含 /models/ 路径（将自动拼接）".to_string());
            }
            if ep.contains(":generateContent") || ep.contains(":streamGenerateContent") {
                return Err(
                    "API 端点不需要包含 :generateContent 路径（将自动拼接）".to_string(),
                );
            }
        }
    }
    if !provider.extra_body.is_object() {
        return Err("自定义参数必须是 JSON 对象".to_string());
    }
    let extra_str = provider.extra_body.to_string();
    if extra_str.len() > 4096 {
        return Err("自定义参数 JSON 不能超过 4KB".to_string());
    }
    Ok(())
}
