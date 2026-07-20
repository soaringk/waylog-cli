pub mod antigravity;
pub mod base;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;

use crate::error::{Result, WaylogError};
use std::sync::Arc;

/// Get a provider by name
pub fn get_provider(name: &str) -> Result<Arc<dyn base::Provider>> {
    match name.to_lowercase().as_str() {
        "antigravity" | "antigravity-cli" => Ok(Arc::new(antigravity::AntigravityProvider::new())),
        "codex" => Ok(Arc::new(codex::CodexProvider::new())),
        "claude" | "claude-code" => Ok(Arc::new(claude::ClaudeProvider::new())),
        "gemini" => Ok(Arc::new(gemini::GeminiProvider::new())),
        "opencode" => Ok(Arc::new(opencode::OpenCodeProvider::new())),
        _ => Err(WaylogError::ProviderNotFound(name.to_string())),
    }
}

/// Get a list of supported provider names
pub fn list_providers() -> Vec<&'static str> {
    vec!["antigravity", "claude", "gemini", "codex", "opencode"]
}
