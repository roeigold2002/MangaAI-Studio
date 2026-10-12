//! Google Gemini API (models/{model}:generateContent) with JSON-schema output.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::*;

pub struct Gemini {
    cfg: ProviderConfig,
    key: String,
    client: reqwest::Client,
}

impl Gemini {
    pub fn new(cfg: ProviderConfig, key: String, client: reqwest::Client) -> Self {
        Self { cfg, key, client }
    }

    fn url(&self) -> String {
        let base = if self.cfg.endpoint.trim().is_empty() {
            "https://generativelanguage.googleapis.com/v1beta"
        } else {
            self.cfg.endpoint.trim()
        };
        format!("{}/models/{}:generateContent", base.trim_end_matches('/'), self.cfg.model)
    }

    pub fn body(&self, req: &ProviderRequest) -> Value {
        let mut parts: Vec<Value> = req
            .images
            .iter()
            .map(|im| json!({"inline_data": {"mime_type": im.mime, "data": b64(&im.data)}}))
            .collect();
        parts.push(json!({"text": req.user}));
        json!({
            "systemInstruction": {"parts": [{"text": req.system}]},
            "contents": [{"role": "user", "parts": parts}],
            "generationConfig": {
                "responseMimeType": "application/json",
                "responseJsonSchema": req.schema,
                "maxOutputTokens": req.max_output_tokens,
            }
        })
    }

    pub fn parse(v: &Value) -> Result<ProviderResponse, ProviderError> {
        if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
            return Err(ProviderError::new(ErrorKind::Refused, format!("request blocked: {reason}")));
        }
        let cand = &v["candidates"][0];
        match cand["finishReason"].as_str().unwrap_or("STOP") {
            "MAX_TOKENS" => return Err(ProviderError::new(ErrorKind::Truncated, "output hit maxOutputTokens")),
            "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII" | "RECITATION" => {
                return Err(ProviderError::new(ErrorKind::Refused, format!("response blocked: {}", cand["finishReason"])))
            }
            _ => {}
        }
        let text: String = cand["content"]["parts"]
            .as_array()
            .map(|ps| {
                ps.iter()
                    .filter(|p| p["thought"] != true)
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            return Err(ProviderError::new(ErrorKind::Refused, "empty response"));
        }
        Ok(ProviderResponse {
            text,
            usage: Usage {
                input_tokens: v["usageMetadata"]["promptTokenCount"].as_u64(),
                output_tokens: v["usageMetadata"]["candidatesTokenCount"].as_u64().map(|c| {
                    c + v["usageMetadata"]["thoughtsTokenCount"].as_u64().unwrap_or(0)
                }),
                reported_cost_usd: None,
            },
            model: v["modelVersion"].as_str().unwrap_or_default().to_string(),
        })
    }
}

#[async_trait]
impl TranslationProvider for Gemini {
    fn id(&self) -> &str {
        &self.cfg.id
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn capabilities(&self) -> Capabilities {
        capabilities_for_kind("gemini", &self.cfg.endpoint)
    }
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let rb = self.client.post(self.url()).header("x-goog-api-key", &self.key).json(&self.body(req));
        Self::parse(&send_json(rb).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_skips_thoughts() {
        let v = json!({
            "candidates": [{"finishReason": "STOP", "content": {"parts": [
                {"text": "thinking...", "thought": true}, {"text": "{\"a\":"}, {"text": "1}"}
            ]}}],
            "usageMetadata": {"promptTokenCount": 7, "candidatesTokenCount": 3, "thoughtsTokenCount": 2}
        });
        let r = Gemini::parse(&v).unwrap();
        assert_eq!(r.text, "{\"a\":1}");
        assert_eq!(r.usage.output_tokens, Some(5));
    }

    #[test]
    fn blocked_prompt() {
        let v = json!({"promptFeedback": {"blockReason": "SAFETY"}});
        assert_eq!(Gemini::parse(&v).unwrap_err().kind, ErrorKind::Refused);
    }

    #[test]
    fn url_and_body() {
        let g = Gemini::new(ProviderConfig { model: "gemini-x".into(), ..Default::default() }, "k".into(), reqwest::Client::new());
        assert!(g.url().ends_with("/models/gemini-x:generateContent"));
        let req = ProviderRequest {
            system: "s".into(),
            user: "u".into(),
            images: vec![],
            schema: json!({"type":"object"}),
            max_output_tokens: 10,
            effort: "low".into(),
        };
        assert_eq!(g.body(&req)["generationConfig"]["responseMimeType"], "application/json");
    }
}
