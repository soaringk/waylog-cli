pub mod antigravity;
pub mod base;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod dsh;
pub mod gemini;
pub mod grok;
mod jsonl;
pub mod opencode;
pub mod qoder;
pub mod qoderwork;

use crate::error::{Result, WaylogError};
use std::sync::Arc;

/// Get a provider by name
pub fn get_provider(name: &str) -> Result<Arc<dyn base::Provider>> {
    match name.to_lowercase().as_str() {
        "antigravity" | "antigravity-cli" => Ok(Arc::new(antigravity::AntigravityProvider::new())),
        "antigravity-2" | "antigravity-2.0" => Ok(Arc::new(
            antigravity::AntigravityProvider::for_surface("antigravity-2", "antigravity"),
        )),
        "antigravity-ide" => Ok(Arc::new(antigravity::AntigravityProvider::for_surface(
            "antigravity-ide",
            "antigravity-ide",
        ))),
        "cursor" => Ok(Arc::new(cursor::CursorProvider::new())),
        "dsh" | "deepseek-harness" => Ok(Arc::new(dsh::DshProvider::new())),
        "grok" | "grok-build" => Ok(Arc::new(grok::GrokProvider::new())),
        "codex" => Ok(Arc::new(codex::CodexProvider::new())),
        "claude" | "claude-code" => Ok(Arc::new(claude::ClaudeProvider::new())),
        "gemini" => Ok(Arc::new(gemini::GeminiProvider::new())),
        "opencode" => Ok(Arc::new(opencode::OpenCodeProvider::new())),
        "qoder" => Ok(Arc::new(qoder::QoderProvider::new())),
        "qoderwork" => Ok(Arc::new(qoderwork::QoderWorkProvider::new())),
        _ => Err(WaylogError::ProviderNotFound(name.to_string())),
    }
}

/// Get a list of supported provider names
pub fn list_providers() -> Vec<&'static str> {
    vec![
        "antigravity",
        "antigravity-2",
        "antigravity-ide",
        "cursor",
        "dsh",
        "grok",
        "claude",
        "gemini",
        "codex",
        "opencode",
        "qoder",
        "qoderwork",
    ]
}
