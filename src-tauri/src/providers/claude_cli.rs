//! Claude Code CLI in headless mode (`claude -p`). Uses the user's existing
//! Claude login, so no API key is stored here. Requests still go to Anthropic.

use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

use super::*;

pub struct ClaudeCli {
    cfg: ProviderConfig,
}

/// GUI apps on macOS get a minimal PATH, so look in the usual install locations too.
pub fn find_claude(explicit: &str) -> Option<PathBuf> {
    if !explicit.trim().is_empty() {
        let p = PathBuf::from(explicit.trim());
        return p.exists().then_some(p);
    }
    if let Ok(p) = which::which("claude") {
        return Some(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

impl ClaudeCli {
    pub fn new(cfg: ProviderConfig) -> Self {
        Self { cfg }
    }

    pub fn parse(stdout: &str) -> Result<ProviderResponse, ProviderError> {
        let v: Value = serde_json::from_str(stdout.trim())
            .map_err(|e| ProviderError::new(ErrorKind::Server, format!("unexpected CLI output: {e}")))?;
        let result_text = v["result"].as_str().unwrap_or_default();
        if v["is_error"] == true || v["subtype"] != "success" {
            let status = v["api_error_status"].as_u64();
            let kind = match status {
                Some(401) | Some(403) => ErrorKind::Auth,
                Some(429) => ErrorKind::RateLimited,
                Some(s) if s >= 500 => ErrorKind::Server,
                _ if result_text.to_lowercase().contains("log in") || result_text.contains("/login") => ErrorKind::Auth,
                _ if v["subtype"] == "error_max_turns" => ErrorKind::Truncated,
                _ if result_text.contains("selected model") || result_text.contains("--model") => ErrorKind::BadRequest,
                _ => ErrorKind::Server,
            };
            let msg: String = result_text.chars().take(300).collect();
            return Err(ProviderError::new(kind, format!("Claude CLI error: {msg}")));
        }
        if v["stop_reason"] == "refusal" {
            return Err(ProviderError::new(ErrorKind::Refused, "the model declined this page"));
        }
        let text = if v["structured_output"].is_object() {
            v["structured_output"].to_string()
        } else {
            result_text.to_string()
        };
        if text.trim().is_empty() {
            return Err(ProviderError::new(ErrorKind::Refused, "empty response"));
        }
        let model = v["modelUsage"].as_object().and_then(|m| m.keys().next().cloned()).unwrap_or_default();
        Ok(ProviderResponse {
            text,
            usage: Usage {
                input_tokens: v["usage"]["input_tokens"].as_u64().map(|i| {
                    i + v["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0)
                        + v["usage"]["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                }),
                output_tokens: v["usage"]["output_tokens"].as_u64(),
                reported_cost_usd: v["total_cost_usd"].as_f64(),
            },
            model,
        })
    }
}

#[async_trait]
impl TranslationProvider for ClaudeCli {
    fn id(&self) -> &str {
        &self.cfg.id
    }
    fn model(&self) -> &str {
        &self.cfg.model
    }
    fn capabilities(&self) -> Capabilities {
        capabilities_for_kind("claude_cli", "")
    }
    async fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let bin = find_claude(&self.cfg.endpoint).ok_or_else(|| {
            ProviderError::new(ErrorKind::Unavailable, "Claude Code CLI not found. Install it or set its path in settings.")
        })?;
        // Images are passed as files the CLI may read; nothing else is accessible.
        let workdir = tempfile::tempdir().map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
        let mut user = req.user.clone();
        let mut image_paths = vec![];
        for (i, im) in req.images.iter().enumerate() {
            let ext = if im.mime.contains("png") { "png" } else if im.mime.contains("webp") { "webp" } else { "jpg" };
            let p = workdir.path().join(format!("page_{}.{ext}", i + 1));
            std::fs::write(&p, &im.data).map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
            image_paths.push(p);
        }
        if !image_paths.is_empty() {
            user.push_str("\n\nPage images (read each one with the Read tool before answering, in this order):\n");
            for p in &image_paths {
                user.push_str(&format!("- {}\n", p.display()));
            }
        }
        let schema = serde_json::to_string(&req.schema).unwrap_or_default();
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("-p")
            .args(["--output-format", "json"])
            .arg("--no-session-persistence")
            .args(["--setting-sources", ""])
            .arg("--strict-mcp-config")
            .arg("--disable-slash-commands")
            .args(["--system-prompt", &req.system])
            .args(["--json-schema", &schema])
            .args(["--effort", &req.effort]);
        if !self.cfg.model.trim().is_empty() {
            cmd.args(["--model", self.cfg.model.trim()]);
        }
        if image_paths.is_empty() {
            cmd.args(["--tools", ""]);
        } else {
            cmd.args(["--tools", "Read"]).args(["--allowedTools", "Read"]);
            cmd.arg("--add-dir").arg(workdir.path());
        }
        cmd.current_dir(workdir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| ProviderError::new(ErrorKind::Unavailable, format!("could not start Claude CLI: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(user.as_bytes()).await.map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
        }
        let timeout = Duration::from_secs(self.cfg.timeout_secs.max(30));
        let out = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| ProviderError::new(ErrorKind::Timeout, "Claude CLI did not finish in time"))?
            .map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if stdout.trim().is_empty() {
            let err: String = String::from_utf8_lossy(&out.stderr).chars().take(300).collect();
            return Err(ProviderError::new(ErrorKind::Unavailable, format!("Claude CLI produced no output: {err}")));
        }
        Self::parse(&stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_output_and_cost() {
        let s = r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"ok\":\"שלום\"}",
            "structured_output":{"ok":"שלום"},"total_cost_usd":0.0012,
            "usage":{"input_tokens":2,"cache_read_input_tokens":100,"output_tokens":54},
            "modelUsage":{"claude-haiku-5-5":{}}}"#;
        let r = ClaudeCli::parse(s).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&r.text).unwrap()["ok"], "שלום");
        assert_eq!(r.usage.reported_cost_usd, Some(0.0012));
        assert_eq!(r.usage.input_tokens, Some(102));
        assert_eq!(r.model, "claude-haiku-5-5");
    }

    #[test]
    fn maps_errors() {
        let s = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        assert_eq!(ClaudeCli::parse(s).unwrap_err().kind, ErrorKind::Auth);
        let s = r#"{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"result":"rate"}"#;
        assert_eq!(ClaudeCli::parse(s).unwrap_err().kind, ErrorKind::RateLimited);
        assert_eq!(ClaudeCli::parse("garbage").unwrap_err().kind, ErrorKind::Server);
        let s = r#"{"type":"result","subtype":"success","is_error":true,"result":"There's an issue with the selected model (x). Run --model to pick a different model."}"#;
        assert_eq!(ClaudeCli::parse(s).unwrap_err().kind, ErrorKind::BadRequest);
    }
}
