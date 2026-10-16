//! Anthropic Messages API (POST /v1/messages) with structured JSON output
//! via `output_config.format`.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::*;

pub struct Anthropic {
    cfg: ProviderConfig,
    key: String,
    client: reqwest::Client,
}

impl Anthropic {
    pub fn new(cfg: ProviderConfig, key: String, client: reqwest::Client) -> Self {
        Self { cfg, key, client }
    }

    fn endpoint(&self) -> String {
        let base = if self.cfg.endpoint.trim().is_empty() { "https://api.anthropic.com" } else { self.cfg.endpoint.trim() };
        format!("{}/v1/messages", base.trim_end_matches('/'))
    }

    /// Models that accept server-side refusal fallback in its "default" form.
    fn supports_default_fallback(&self) -> bool {
        let official = self.cfg.endpoint.trim().is_empty() || self.cfg.endpoint.contains("api.anthropic.com");
        official
            && matches!(self.cfg.model.as_str(), "claude-opus-5-5" | "claude-opus-5" | "claude-fable-5-1" | "claude-sonnet-5-5")
    }

    pub fn body(&self, req: &ProviderRequest) -> Value {
        let mut content: Vec<Value> = req
            .images
            .iter()
            .map(|im| json!({"type": "image", "source": {"type": "base64", "media_type": im.mime, "data": b64(&im.data)}}))
            .collect();
        content.push(json!({"type": "text", "text": req.user}));
        let mut body = json!({
            "model": self.cfg.model,
            "max_tokens": req.max_output_tokens.max(1024),
            // The system prompt is identical for every page of a chapter: a cache breakpoint
            // after it turns its tokens into cheap cache reads on all but the first request.
            "system": [{"type": "text", "text": req.system, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": content}],
            "output_config": {
                "effort": req.effort,
                "format": {"type": "json_schema", "schema": req.schema},
            },
        });
        if self.supports_default_fallback() {
            body["fallbacks"] = json!("default");
        }
        body
    }

    pub fn parse(v: &Value) -> Result<ProviderResponse, ProviderError> {
        let stop = v["stop_reason"].as_str().unwrap_or("");
        if stop == "refusal" {
            let cat = v["stop_details"]["category"].as_str().unwrap_or("unspecified");
            return Err(ProviderError::new(ErrorKind::Refused, format!("the model declined this page (category: {cat})")));
        }
        if stop == "max_tokens" {
            return Err(ProviderError::new(ErrorKind::Truncated, "output hit max_tokens before the JSON was complete"));
        }
        let text = v["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text").and_then(|b| b["text"].as_str()))
            .ok_or_else(|| ProviderError::new(ErrorKind::Refused, "response had no text content"))?;
        Ok(ProviderResponse {
            text: text.to_string(),
            usage: Usage {
                input_tokens: v["usage"]["input_tokens"].as_u64().map(|i| {
                    i + v["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0)
                        + v["usage"]["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                }),
                output_tokens: v["usage"]["output_tokens"].as_u64(),
                reported_cost_usd: None,
            },
            model: v["model"].as_str().unwrap_or_default().to_string(),
        })
    }
}

#[async_trait]
impl TranslationProvider for Anthropic {
    fn id(&self) -> &str {
        &self.cfg.id
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn capabilities(&self) -> Capabilities {
        capabilities_for_kind("anthropic", &self.cfg.endpoint)
    }
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let mut rb = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.body(req));
        if self.supports_default_fallback() {
            rb = rb.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let v = send_json(rb).await?;
        Self::parse(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req() -> ProviderRequest {
        ProviderRequest {
            system: "sys".into(),
            user: "hello".into(),
            images: vec![ImagePart { mime: "image/png".into(), data: vec![1, 2, 3] }],
            schema: json!({"type": "object"}),
            max_output_tokens: 4000,
            effort: "medium".into(),
            thinking_budget: None,
        }
    }

    fn cfg(endpoint: &str) -> ProviderConfig {
        ProviderConfig { id: "anthropic".into(), kind: "anthropic".into(), model: "claude-opus-5-5".into(), endpoint: endpoint.into(), ..Default::default() }
    }

    #[test]
    fn body_shape() {
        let a = Anthropic::new(cfg(""), "k".into(), reqwest::Client::new());
        let b = a.body(&req());
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["system"][0]["text"], "sys");
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["messages"][0]["content"][0]["source"]["data"], "AQID");
        assert_eq!(b["messages"][0]["content"][1]["text"], "hello");
        assert_eq!(b["fallbacks"], "default");
        // proxies / custom endpoints don't get the beta parameter
        let a = Anthropic::new(cfg("http://127.0.0.1:9"), "k".into(), reqwest::Client::new());
        assert!(a.body(&req()).get("fallbacks").is_none());
    }

    #[tokio::test]
    async fn success_and_errors_against_mock_server() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "good"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "claude-opus-5-5",
                "stop_reason": "end_turn",
                "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "{\"a\":1}"}],
                "usage": {"input_tokens": 10, "output_tokens": 5}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "bad"))
            .respond_with(ResponseTemplate::new(401).set_body_string("{\"error\":\"invalid x-api-key\"}"))
            .mount(&server)
            .await;
        let ok = Anthropic::new(cfg(&server.uri()), "good".into(), reqwest::Client::new());
        let r = ok.complete(&req()).await.unwrap();
        assert_eq!(r.text, "{\"a\":1}");
        assert_eq!(r.usage.input_tokens, Some(10));
        let bad = Anthropic::new(cfg(&server.uri()), "bad".into(), reqwest::Client::new());
        let e = bad.complete(&req()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Auth);
        assert!(!e.message.contains("bad\""));
    }

    #[test]
    fn refusal_and_truncation() {
        let r = Anthropic::parse(&json!({"stop_reason": "refusal", "stop_details": {"category": "cyber"}, "content": []}));
        assert_eq!(r.unwrap_err().kind, ErrorKind::Refused);
        let r = Anthropic::parse(&json!({"stop_reason": "max_tokens", "content": [{"type":"text","text":"{"}]}));
        assert_eq!(r.unwrap_err().kind, ErrorKind::Truncated);
    }
}
