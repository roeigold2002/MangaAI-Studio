//! OpenAI Chat Completions API, also used for OpenAI-compatible servers
//! (LM Studio, vLLM, llama.cpp server, OpenRouter...).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::*;

pub struct OpenAi {
    cfg: ProviderConfig,
    key: Option<String>,
    client: reqwest::Client,
    official: bool,
}

impl OpenAi {
    pub fn new(cfg: ProviderConfig, key: Option<String>, client: reqwest::Client, official: bool) -> Self {
        Self { cfg, key, client, official }
    }

    fn url(&self) -> String {
        let base = if self.cfg.endpoint.trim().is_empty() { "https://api.openai.com/v1" } else { self.cfg.endpoint.trim() };
        format!("{}/chat/completions", base.trim_end_matches('/'))
    }

    pub fn body(&self, req: &ProviderRequest, structured: bool) -> Value {
        let mut parts = vec![json!({"type": "text", "text": req.user})];
        for im in &req.images {
            parts.push(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{};base64,{}", im.mime, b64(&im.data))}
            }));
        }
        let mut body = json!({
            "model": self.cfg.model,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": parts},
            ],
        });
        if self.official {
            body["max_completion_tokens"] = json!(req.max_output_tokens);
        } else {
            body["max_tokens"] = json!(req.max_output_tokens);
        }
        if structured {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {"name": "manga_translation", "schema": req.schema, "strict": true}
            });
        }
        body
    }

    pub fn parse(v: &Value) -> Result<ProviderResponse, ProviderError> {
        let choice = &v["choices"][0];
        if let Some(r) = choice["message"]["refusal"].as_str().filter(|r| !r.is_empty()) {
            return Err(ProviderError::new(ErrorKind::Refused, format!("the model refused: {r}")));
        }
        if choice["finish_reason"] == "length" {
            return Err(ProviderError::new(ErrorKind::Truncated, "output hit the token limit"));
        }
        let text = choice["message"]["content"]
            .as_str()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| ProviderError::new(ErrorKind::Refused, "empty response"))?;
        Ok(ProviderResponse {
            text: text.to_string(),
            usage: Usage {
                input_tokens: v["usage"]["prompt_tokens"].as_u64(),
                output_tokens: v["usage"]["completion_tokens"].as_u64(),
                reported_cost_usd: None,
            },
            model: v["model"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn post(&self, body: &Value) -> Result<Value, ProviderError> {
        let mut rb = self.client.post(self.url()).json(body);
        if let Some(k) = &self.key {
            rb = rb.bearer_auth(k);
        }
        send_json(rb).await
    }
}

#[async_trait]
impl TranslationProvider for OpenAi {
    fn id(&self) -> &str {
        &self.cfg.id
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn capabilities(&self) -> Capabilities {
        capabilities_for_kind(if self.official { "openai" } else { "openai_compatible" }, &self.cfg.endpoint)
    }
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        match self.post(&self.body(req, true)).await {
            Ok(v) => Self::parse(&v),
            // Some compatible servers reject json_schema; retry once relying on the prompt.
            Err(e) if !self.official && e.kind == ErrorKind::BadRequest => {
                tracing::warn!("compatible endpoint rejected json_schema, retrying without it: {}", e.message);
                Self::parse(&self.post(&self.body(req, false)).await?)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req() -> ProviderRequest {
        ProviderRequest {
            system: "s".into(),
            user: "u".into(),
            images: vec![ImagePart { mime: "image/jpeg".into(), data: vec![0xff] }],
            schema: json!({"type": "object"}),
            max_output_tokens: 100,
            effort: "medium".into(),
            thinking_budget: None,
        }
    }

    #[tokio::test]
    async fn compatible_falls_back_without_schema() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_partial_json(json!({"response_format": {"type": "json_schema"}})))
            .respond_with(ResponseTemplate::new(400).set_body_string("response_format not supported"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "local", "choices": [{"message": {"content": "{\"x\":1}"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 4}
            })))
            .mount(&server)
            .await;
        let cfg = ProviderConfig { id: "c".into(), model: "local".into(), endpoint: format!("{}/v1", server.uri()), ..Default::default() };
        let p = OpenAi::new(cfg, None, reqwest::Client::new(), false);
        let r = p.complete(&req()).await.unwrap();
        assert_eq!(r.text, "{\"x\":1}");
        assert_eq!(r.usage.output_tokens, Some(4));
    }

    #[test]
    fn image_as_data_url_and_length_finish() {
        let p = OpenAi::new(ProviderConfig::default(), None, reqwest::Client::new(), true);
        let b = p.body(&req(), true);
        assert!(b["messages"][1]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/jpeg;base64,"));
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        let e = OpenAi::parse(&json!({"choices": [{"message": {"content": "{"}, "finish_reason": "length"}]})).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Truncated);
    }
}
