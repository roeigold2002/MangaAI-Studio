//! Ollama native chat API (POST /api/chat) with `format` = JSON schema.
//! Runs fully on this computer when the endpoint is localhost.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::*;

pub struct Ollama {
    cfg: ProviderConfig,
    client: reqwest::Client,
}

impl Ollama {
    pub fn new(cfg: ProviderConfig, client: reqwest::Client) -> Self {
        Self { cfg, client }
    }

    fn base(&self) -> String {
        let b = if self.cfg.endpoint.trim().is_empty() { "http://127.0.0.1:11434" } else { self.cfg.endpoint.trim() };
        b.trim_end_matches('/').to_string()
    }

    pub fn body(&self, req: &ProviderRequest) -> Value {
        let mut user = json!({"role": "user", "content": req.user});
        if !req.images.is_empty() {
            user["images"] = json!(req.images.iter().map(|i| b64(&i.data)).collect::<Vec<_>>());
        }
        json!({
            "model": self.cfg.model,
            "stream": false,
            "format": req.schema,
            "messages": [{"role": "system", "content": req.system}, user],
            "options": {"num_ctx": 16384, "num_predict": req.max_output_tokens, "temperature": 0.3},
        })
    }

    pub fn parse(v: &Value) -> Result<ProviderResponse, ProviderError> {
        if v["done_reason"] == "length" {
            return Err(ProviderError::new(ErrorKind::Truncated, "output hit num_predict"));
        }
        let text = v["message"]["content"]
            .as_str()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| ProviderError::new(ErrorKind::Refused, "empty response"))?;
        Ok(ProviderResponse {
            text: text.to_string(),
            usage: Usage {
                input_tokens: v["prompt_eval_count"].as_u64(),
                output_tokens: v["eval_count"].as_u64(),
                reported_cost_usd: Some(0.0),
            },
            model: v["model"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Installed model names (for the settings UI).
    pub async fn list_models(endpoint: &str) -> Result<Vec<String>, ProviderError> {
        let client = http_client(10)?;
        let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
        let v = send_json(client.get(url)).await?;
        Ok(v["models"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
            .unwrap_or_default())
    }
}

#[async_trait]
impl TranslationProvider for Ollama {
    fn id(&self) -> &str {
        &self.cfg.id
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn capabilities(&self) -> Capabilities {
        capabilities_for_kind("ollama", &self.cfg.endpoint)
    }
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let rb = self.client.post(format!("{}/api/chat", self.base())).json(&self.body(req));
        match send_json(rb).await {
            Err(e) if e.kind == ErrorKind::Network => Err(ProviderError::new(
                ErrorKind::Unavailable,
                format!("Ollama is not reachable at {} — is it running? ({})", self.base(), e.message),
            )),
            r => Self::parse(&r?),
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ok_and_length() {
        let r = Ollama::parse(&json!({"model": "m", "message": {"content": "{}"}, "prompt_eval_count": 5, "eval_count": 2})).unwrap();
        assert_eq!(r.usage.input_tokens, Some(5));
        assert_eq!(Ollama::parse(&json!({"done_reason": "length", "message": {"content": "{"}})).unwrap_err().kind, ErrorKind::Truncated);
    }

    #[tokio::test]
    async fn unreachable_is_unavailable() {
        let cfg = ProviderConfig { model: "m".into(), endpoint: "http://127.0.0.1:9".into(), ..Default::default() };
        let p = Ollama::new(cfg, reqwest::Client::new());
        let req = ProviderRequest {
            system: String::new(),
            user: String::new(),
            images: vec![],
            schema: json!({}),
            max_output_tokens: 10,
            effort: "low".into(),
        };
        assert_eq!(p.complete(&req).await.unwrap_err().kind, ErrorKind::Unavailable);
    }
}
