//! Translation provider adapters. Every adapter takes the same request
//! (system prompt, user text, optional images, JSON schema) and returns raw
//! JSON text plus usage. Validation of that JSON happens in `translate`.

mod anthropic;
mod claude_cli;
mod gemini;
mod ollama;
pub use claude_cli::find_claude;
pub use ollama::Ollama;
mod openai;

use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

use crate::secrets::SecretStore;
use crate::settings::ProviderConfig;

#[derive(Debug, Clone)]
pub struct ImagePart {
    pub mime: String,
    pub data: Vec<u8>,
    /// File on disk (used by the Claude CLI adapter, which reads files itself).
    pub path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub system: String,
    pub user: String,
    pub images: Vec<ImagePart>,
    pub schema: serde_json::Value,
    pub max_output_tokens: u32,
    /// low | medium | high (mapped per provider where supported)
    pub effort: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Cost reported by the provider itself (Claude CLI). Otherwise computed from prices.
    pub reported_cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: String,
    pub usage: Usage,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Missing or rejected credentials. Not retryable.
    Auth,
    RateLimited,
    Timeout,
    Network,
    /// 5xx / overloaded.
    Server,
    /// 4xx other than auth/rate: bad model name, bad request. Not retryable.
    BadRequest,
    /// Model refused or returned no usable content.
    Refused,
    /// Output cut off (max tokens).
    Truncated,
    /// Provider not installed / not configured.
    Unavailable,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip)]
    pub retry_after: Option<Duration>,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), retry_after: None }
    }
    pub fn retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::RateLimited | ErrorKind::Timeout | ErrorKind::Network | ErrorKind::Server | ErrorKind::Truncated
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub vision: bool,
    pub local: bool,
    pub needs_key: bool,
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    fn id(&self) -> &str;
    fn model(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError>;
}

pub fn capabilities_for_kind(kind: &str, endpoint: &str) -> Capabilities {
    let local_endpoint = endpoint.contains("127.0.0.1") || endpoint.contains("localhost") || endpoint.contains("[::1]");
    match kind {
        "anthropic" | "openai" | "gemini" => Capabilities { vision: true, local: false, needs_key: true },
        "claude_cli" => Capabilities { vision: true, local: false, needs_key: false },
        "ollama" => Capabilities { vision: true, local: local_endpoint, needs_key: false },
        _ => Capabilities { vision: true, local: local_endpoint, needs_key: false },
    }
}

pub fn build(cfg: &ProviderConfig, secrets: &dyn SecretStore) -> Result<Arc<dyn TranslationProvider>, ProviderError> {
    let key = || -> Result<String, ProviderError> {
        secrets
            .get(&cfg.id)
            .map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.message))?
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| ProviderError::new(ErrorKind::Auth, format!("no API key configured for {}", cfg.label)))
    };
    let optional_key = || secrets.get(&cfg.id).ok().flatten().filter(|k| !k.trim().is_empty());
    if cfg.model.trim().is_empty() && cfg.kind != "claude_cli" {
        return Err(ProviderError::new(ErrorKind::BadRequest, format!("no model selected for {}", cfg.label)));
    }
    let client = http_client(cfg.timeout_secs)?;
    Ok(match cfg.kind.as_str() {
        "anthropic" => Arc::new(anthropic::Anthropic::new(cfg.clone(), key()?, client)),
        "openai" => Arc::new(openai::OpenAi::new(cfg.clone(), Some(key()?), client, true)),
        "openai_compatible" => Arc::new(openai::OpenAi::new(cfg.clone(), optional_key(), client, false)),
        "gemini" => Arc::new(gemini::Gemini::new(cfg.clone(), key()?, client)),
        "ollama" => Arc::new(ollama::Ollama::new(cfg.clone(), client)),
        "claude_cli" => Arc::new(claude_cli::ClaudeCli::new(cfg.clone())),
        other => return Err(ProviderError::new(ErrorKind::Unavailable, format!("unknown provider kind: {other}"))),
    })
}

fn http_client(timeout_secs: u64) -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(10)))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("MangaAI-Studio/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))
}

/// Map transport errors.
pub(crate) fn map_reqwest(e: reqwest::Error) -> ProviderError {
    if e.is_timeout() {
        ProviderError::new(ErrorKind::Timeout, "the provider did not answer in time")
    } else if e.is_connect() {
        ProviderError::new(ErrorKind::Network, format!("could not connect: {e}"))
    } else {
        ProviderError::new(ErrorKind::Network, e.to_string())
    }
}

/// Map an HTTP error status + body into a ProviderError (body is truncated, never contains our key).
pub(crate) fn map_status(status: u16, headers: &reqwest::header::HeaderMap, body: &str) -> ProviderError {
    let snippet: String = body.chars().take(400).collect();
    let retry_after = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs);
    let kind = match status {
        401 | 403 => ErrorKind::Auth,
        429 => ErrorKind::RateLimited,
        408 => ErrorKind::Timeout,
        500..=599 => ErrorKind::Server,
        _ => ErrorKind::BadRequest,
    };
    // Anthropic uses 529 for overloaded; covered by 5xx.
    let mut err = ProviderError::new(kind, format!("HTTP {status}: {snippet}"));
    err.retry_after = retry_after;
    err
}

pub(crate) async fn send_json(
    rb: reqwest::RequestBuilder,
) -> Result<serde_json::Value, ProviderError> {
    let resp = rb.send().await.map_err(map_reqwest)?;
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body = resp.text().await.map_err(map_reqwest)?;
    if !(200..300).contains(&status) {
        return Err(map_status(status, &headers, &body));
    }
    serde_json::from_str(&body)
        .map_err(|e| ProviderError::new(ErrorKind::Server, format!("provider returned invalid JSON envelope: {e}")))
}

pub(crate) fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Cost in USD from token usage and configured prices (None if prices unknown).
pub fn cost_from_usage(cfg: &ProviderConfig, usage: &Usage) -> Option<f64> {
    if let Some(c) = usage.reported_cost_usd {
        return Some(c);
    }
    if cfg.price_input_per_mtok <= 0.0 && cfg.price_output_per_mtok <= 0.0 {
        return None;
    }
    let i = usage.input_tokens? as f64;
    let o = usage.output_tokens.unwrap_or(0) as f64;
    Some(i / 1e6 * cfg.price_input_per_mtok + o / 1e6 * cfg.price_output_per_mtok)
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use parking_lot::Mutex;

    /// Scripted provider for tests: returns queued responses in order and records requests.
    pub struct MockProvider {
        pub responses: Mutex<std::collections::VecDeque<Result<String, ProviderError>>>,
        pub requests: Mutex<Vec<ProviderRequest>>,
    }

    impl MockProvider {
        pub fn new(responses: Vec<Result<String, ProviderError>>) -> Arc<Self> {
            Arc::new(Self { responses: Mutex::new(responses.into()), requests: Mutex::new(vec![]) })
        }
    }

    #[async_trait]
    impl TranslationProvider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn model(&self) -> &str {
            "mock-model"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { vision: true, local: true, needs_key: false }
        }
        async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
            self.requests.lock().push(req.clone());
            let next = self
                .responses
                .lock()
                .pop_front()
                .unwrap_or_else(|| Err(ProviderError::new(ErrorKind::Server, "mock exhausted")));
            next.map(|text| ProviderResponse {
                text,
                usage: Usage { input_tokens: Some(1000), output_tokens: Some(200), reported_cost_usd: None },
                model: "mock-model".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;

    #[test]
    fn build_requires_key_for_cloud_providers() {
        let s = MemoryStore::default();
        let cfg = crate::settings::default_providers().into_iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(build(&cfg, &s).err().unwrap().kind, ErrorKind::Auth);
        s.set("anthropic", "test-key").unwrap();
        assert_eq!(build(&cfg, &s).unwrap().model(), "claude-opus-5-5");
    }

    #[test]
    fn build_requires_model() {
        let s = MemoryStore::default();
        let cfg = crate::settings::default_providers().into_iter().find(|p| p.id == "ollama").unwrap();
        assert_eq!(build(&cfg, &s).err().unwrap().kind, ErrorKind::BadRequest);
    }

    #[test]
    fn status_mapping() {
        let h = reqwest::header::HeaderMap::new();
        assert_eq!(map_status(401, &h, "").kind, ErrorKind::Auth);
        assert_eq!(map_status(429, &h, "").kind, ErrorKind::RateLimited);
        assert_eq!(map_status(529, &h, "").kind, ErrorKind::Server);
        assert_eq!(map_status(400, &h, "").kind, ErrorKind::BadRequest);
        assert!(map_status(503, &h, "").retryable());
        assert!(!map_status(401, &h, "").retryable());
    }

    #[test]
    fn cost_math() {
        let mut cfg = ProviderConfig::default();
        let u = Usage { input_tokens: Some(1_000_000), output_tokens: Some(100_000), reported_cost_usd: None };
        assert_eq!(cost_from_usage(&cfg, &u), None);
        cfg.price_input_per_mtok = 4.0;
        cfg.price_output_per_mtok = 20.0;
        assert!((cost_from_usage(&cfg, &u).unwrap() - 6.0).abs() < 1e-9);
    }
}
