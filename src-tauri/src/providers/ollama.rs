//! Ollama native chat API (POST /api/chat) with `format` = JSON schema.
//! Runs fully on this computer when the endpoint is localhost.

use async_trait::async_trait;
use serde_json::{json, Value};

use super::*;

pub struct Ollama {
    cfg: ProviderConfig,
    client: reqwest::Client,
    auto_layers: tokio::sync::OnceCell<Option<u32>>,
}

/// VRAM kept free for the OCR/inpainting worker, the desktop and Ollama's compute buffers.
const WORKER_RESERVE_MIB: u64 = 3584;

/// Layers to keep on the GPU when the model would not fit next to the worker, else None (Ollama
/// decides). Over-committing VRAM on Windows does not fail: WDDM pages it to system RAM and
/// generation drops from ~80 to ~3 tokens/s, so a few layers on the CPU are much faster.
pub fn auto_gpu_layers(vram_mib: u64, model_bytes: u64, block_count: u64) -> Option<u32> {
    let budget = vram_mib.checked_sub(WORKER_RESERVE_MIB)?;
    let size_mib = model_bytes / (1 << 20);
    if size_mib <= budget || block_count == 0 {
        return None;
    }
    let per_layer = size_mib / (block_count + 1); // + the output layer
    Some((budget / per_layer.max(1)).clamp(1, block_count) as u32)
}

impl Ollama {
    pub fn new(cfg: ProviderConfig, client: reqwest::Client) -> Self {
        Self { cfg, client, auto_layers: tokio::sync::OnceCell::new() }
    }

    /// The configured layer count, or one worked out from this PC's GPU for a local Ollama.
    async fn num_gpu(&self) -> Option<u32> {
        if self.cfg.gpu_layers > 0 {
            return Some(self.cfg.gpu_layers);
        }
        if !capabilities_for_kind("ollama", &self.cfg.endpoint).local {
            return None;
        }
        *self.auto_layers.get_or_init(|| self.probe_layers()).await
    }

    async fn probe_layers(&self) -> Option<u32> {
        let vram = crate::platform::nvidia_vram_mib().await?;
        let tags = send_json(self.client.get(format!("{}/api/tags", self.base()))).await.ok()?;
        let want = if self.cfg.model.contains(':') { self.cfg.model.clone() } else { format!("{}:latest", self.cfg.model) };
        let size = tags["models"].as_array()?.iter().find(|m| m["name"] == want.as_str())?["size"].as_u64()?;
        let show = send_json(self.client.post(format!("{}/api/show", self.base())).json(&json!({"model": self.cfg.model}))).await.ok()?;
        let blocks = show["model_info"].as_object()?.iter().find(|(k, _)| k.ends_with(".block_count"))?.1.as_u64()?;
        let n = auto_gpu_layers(vram, size, blocks);
        tracing::info!("ollama {}: {vram} MiB VRAM, {} MiB model, {blocks} layers -> num_gpu {n:?}", self.cfg.model, size >> 20);
        n
    }

    fn base(&self) -> String {
        let b = if self.cfg.endpoint.trim().is_empty() { "http://127.0.0.1:11434" } else { self.cfg.endpoint.trim() };
        b.trim_end_matches('/').to_string()
    }

    /// Thinking models (Gemma 4, Qwen 3.x, …) otherwise think by default: on a manga page that
    /// is a minute of hidden tokens that also eat `num_predict`, so the answer gets cut off.
    /// Only the "best" quality profile lets the model think.
    pub fn body(&self, req: &ProviderRequest) -> Value {
        self.body_with(req, req.effort == "high", true, (self.cfg.gpu_layers > 0).then_some(self.cfg.gpu_layers))
    }

    fn body_with(&self, req: &ProviderRequest, think: bool, images: bool, num_gpu: Option<u32>) -> Value {
        let mut user = json!({"role": "user", "content": req.user});
        if images && !req.images.is_empty() {
            user["images"] = json!(req.images.iter().map(|i| b64(&i.data)).collect::<Vec<_>>());
        }
        let mut options = json!({"num_ctx": 16384, "num_predict": req.max_output_tokens, "temperature": 0.3});
        if let Some(n) = num_gpu {
            options["num_gpu"] = json!(n);
        }
        json!({
            "model": self.cfg.model,
            "stream": false,
            "format": req.schema,
            "think": think,
            "messages": [{"role": "system", "content": req.system}, user],
            "options": options,
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
        let (mut think, mut images, num_gpu) = (req.effort == "high", true, self.num_gpu().await);
        let body = self.body_with(req, think, images, num_gpu);
        let mut res = send_json(self.client.post(format!("{}/api/chat", self.base())).json(&body)).await;
        // A model without the feature answers 400 "does not support thinking" / "… multimodal
        // requests": drop what it lacks and ask again (text-only models then translate from OCR).
        while let Err(e) = &res {
            let msg = e.message.to_lowercase();
            if think && msg.contains("think") {
                think = false;
            } else if images && !req.images.is_empty() && (msg.contains("multimodal") || msg.contains("image")) {
                images = false;
            } else {
                break;
            }
            let body = self.body_with(req, think, images, num_gpu);
            res = send_json(self.client.post(format!("{}/api/chat", self.base())).json(&body)).await;
        }
        match res {
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

    fn req(effort: &str) -> ProviderRequest {
        ProviderRequest {
            system: "s".into(),
            user: "u".into(),
            images: vec![],
            schema: json!({}),
            max_output_tokens: 10,
            effort: effort.into(),
            thinking_budget: None,
        }
    }

    #[test]
    fn thinking_only_for_best_quality() {
        let p = Ollama::new(ProviderConfig { model: "m".into(), ..Default::default() }, reqwest::Client::new());
        assert_eq!(p.body(&req("medium"))["think"], false);
        assert_eq!(p.body(&req("low"))["think"], false);
        assert_eq!(p.body(&req("high"))["think"], true);
        assert!(p.body(&req("low"))["options"].get("num_gpu").is_none());
        let p = Ollama::new(ProviderConfig { model: "m".into(), gpu_layers: 26, ..Default::default() }, reqwest::Client::new());
        assert_eq!(p.body(&req("low"))["options"]["num_gpu"], 26);
    }

    #[tokio::test]
    async fn model_without_thinking_is_retried_without_it() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat")).and(body_partial_json(json!({"think": true})))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "\"m\" does not support thinking"})))
            .expect(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/chat")).and(body_partial_json(json!({"think": false})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"model": "m", "message": {"content": "{}"}})))
            .expect(1).mount(&server).await;
        let p = Ollama::new(ProviderConfig { model: "m".into(), endpoint: server.uri(), ..Default::default() }, reqwest::Client::new());
        assert_eq!(p.complete(&req("high")).await.unwrap().text, "{}");
    }

    #[test]
    fn layers_only_when_the_model_does_not_fit() {
        // gemma4:26b QAT on a 16 GB card: measured fast at 26 layers, paged at all 31
        assert_eq!(auto_gpu_layers(16303, 15_634_199_946, 30), Some(26));
        assert_eq!(auto_gpu_layers(16303, 8_021_618_941, 48), None);
        assert_eq!(auto_gpu_layers(24564, 15_634_199_946, 30), None);
        assert_eq!(auto_gpu_layers(2048, 15_634_199_946, 30), None);
    }

    #[tokio::test]
    async fn text_only_model_is_retried_without_images() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat")).and(body_string_contains("\"images\""))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "Multimodal data provided, but model does not support multimodal requests."})))
            .expect(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"model": "m", "message": {"content": "{}"}})))
            .expect(1).mount(&server).await;
        let p = Ollama::new(ProviderConfig { model: "m".into(), endpoint: server.uri(), ..Default::default() }, reqwest::Client::new());
        let mut r = req("medium");
        r.images = vec![ImagePart { mime: "image/png".into(), data: vec![1, 2, 3] }];
        assert_eq!(p.complete(&r).await.unwrap().text, "{}");
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
            thinking_budget: None,
        };
        assert_eq!(p.complete(&req).await.unwrap_err().kind, ErrorKind::Unavailable);
    }
}
