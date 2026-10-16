//! Claude Code CLI in headless mode (`claude -p`). Uses the user's existing
//! Claude login, so no API key is stored here. Requests still go to Anthropic.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::*;

pub struct ClaudeCli {
    cfg: ProviderConfig,
}

/// GUI apps on macOS get a minimal PATH, so look in the usual install locations too.
/// On Windows an npm `claude.cmd` shim is resolved to the program it starts: batch files
/// run through cmd.exe, which cannot pass multi-line or quoted arguments reliably.
pub fn find_claude(explicit: &str) -> Option<PathBuf> {
    if !explicit.trim().is_empty() {
        let p = PathBuf::from(explicit.trim());
        return p.exists().then(|| resolve_shim(&p).unwrap_or(p));
    }
    if let Ok(p) = which::which("claude") {
        if let Some(target) = resolve_shim(&p) {
            return Some(target);
        }
        if !is_batch(&p) {
            return Some(p);
        }
    }
    let home = crate::platform::home_dir();
    let mut candidates = vec![
        home.join(".local/bin").join(crate::platform::exe("claude")),
        home.join(".claude/local").join(crate::platform::exe("claude")),
    ];
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            let pkg = appdata.join("npm/node_modules/@anthropic-ai/claude-code");
            candidates.push(pkg.join("bin/claude.exe"));
            candidates.push(pkg.join("cli.js"));
        }
    } else {
        candidates.extend([PathBuf::from("/opt/homebrew/bin/claude"), PathBuf::from("/usr/local/bin/claude")]);
    }
    candidates.into_iter().find(|p| p.exists())
}

fn is_batch(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// The program an npm-style `.cmd` shim launches (`"%dp0%\node_modules\…\claude.exe" %*`,
/// or `cli.js` under node in older shims). None if `p` is not a shim or its target is gone.
pub fn resolve_shim(p: &Path) -> Option<PathBuf> {
    if !is_batch(p) {
        return None;
    }
    let text = std::fs::read_to_string(p).ok()?;
    shim_target(&text, p.parent()?)
}

fn shim_target(text: &str, dir: &Path) -> Option<PathBuf> {
    text.split('"')
        .map(str::trim)
        .filter(|t| {
            let l = t.to_ascii_lowercase();
            (l.starts_with("%dp0%") || l.starts_with("%~dp0")) && [".exe", ".js", ".cjs", ".mjs"].iter().any(|x| l.ends_with(x))
        })
        .map(|t| {
            let rel = t.trim_start_matches("%dp0%").trim_start_matches("%~dp0").trim_start_matches(['\\', '/']);
            rel.split(['\\', '/']).filter(|c| !c.is_empty()).fold(dir.to_path_buf(), |acc, c| acc.join(c))
        })
        .find(|t| t.exists())
}

/// Program and leading arguments that start what `find_claude` returned (a JS entry runs under node).
fn launcher(found: &Path) -> Result<(PathBuf, Vec<std::ffi::OsString>), ProviderError> {
    let ext = found.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(ext.as_str(), "js" | "cjs" | "mjs") {
        let node = which::which("node")
            .map_err(|_| ProviderError::new(ErrorKind::Unavailable, "Claude CLI needs Node.js, which was not found"))?;
        return Ok((node, vec![found.as_os_str().to_owned()]));
    }
    if is_batch(found) {
        return Err(ProviderError::new(
            ErrorKind::Unavailable,
            format!("{} is a batch file; set the path to claude.exe in the provider settings", found.display()),
        ));
    }
    Ok((found.to_path_buf(), vec![]))
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
        let workdir = tempfile::tempdir().map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
        let schema = serde_json::to_string(&req.schema).unwrap_or_default();
        // The system prompt is long and multi-line: pass it as a file, not an argument
        // (Windows command lines are length-limited and quoting newlines is unreliable).
        let system_file = workdir.path().join("system-prompt.txt");
        std::fs::write(&system_file, &req.system).map_err(|e| ProviderError::new(ErrorKind::Unavailable, e.to_string()))?;
        let (program, prefix) = launcher(&bin)?;
        let mut cmd = tokio::process::Command::new(&program);
        crate::platform::no_window(&mut cmd);
        // Page images go inline in one stream-json user message, so the model sees them in
        // its first turn: no tools at all (no Read round trips, nothing on disk readable).
        cmd.args(&prefix)
            .arg("-p")
            .args(["--input-format", "stream-json"])
            .args(["--output-format", "stream-json"])
            .arg("--verbose")
            .arg("--no-session-persistence")
            .args(["--setting-sources", ""])
            .arg("--strict-mcp-config")
            .arg("--disable-slash-commands")
            .args(["--tools", ""])
            .arg("--system-prompt-file")
            .arg(&system_file)
            .args(["--json-schema", &schema])
            .args(["--effort", &req.effort]);
        if !self.cfg.model.trim().is_empty() {
            cmd.args(["--model", self.cfg.model.trim()]);
        }
        // Budget-thinking models (the `haiku` alias) otherwise think for minutes per page;
        // adaptive-thinking models ignore this and follow --effort.
        if let Some(b) = req.thinking_budget {
            cmd.env("MAX_THINKING_TOKENS", b.to_string());
        }
        cmd.current_dir(workdir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let message = user_message_line(req);
        let started = std::time::Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| ProviderError::new(ErrorKind::Unavailable, format!("could not start Claude CLI: {e}")))?;
        let mut stdin = child.stdin.take();
        // write in its own task: the CLI may print before it has read all of stdin
        let writer = tokio::spawn(async move {
            if let Some(stdin) = stdin.as_mut() {
                stdin.write_all(message.as_bytes()).await?;
                stdin.shutdown().await?;
            }
            drop(stdin);
            Ok::<_, std::io::Error>(())
        });
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let read = async {
            let mut events = vec![];
            let mut result = None;
            if let Some(out) = stdout {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let at = started.elapsed().as_secs_f32();
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    if v["subtype"] != "thinking_tokens" {
                        events.push(format!("{at:.2}s {}", event_label(&v)));
                    }
                    if v["type"] == "result" {
                        result = Some(line);
                    }
                }
            }
            let mut err = String::new();
            if let Some(mut e) = stderr {
                let _ = e.read_to_string(&mut err).await;
            }
            let _ = child.wait().await;
            (result, events, err)
        };
        let timeout = Duration::from_secs(self.cfg.timeout_secs.max(30));
        let (result, events, err) = tokio::time::timeout(timeout, read)
            .await
            .map_err(|_| ProviderError::new(ErrorKind::Timeout, "Claude CLI did not finish in time"))?;
        let _ = writer.await;
        tracing::debug!(model = %self.cfg.model, secs = started.elapsed().as_secs_f32(), "Claude CLI timeline: {}", events.join(" | "));
        let Some(result) = result else {
            let err: String = err.chars().take(300).collect();
            return Err(ProviderError::new(ErrorKind::Unavailable, format!("Claude CLI produced no output: {err}")));
        };
        Self::parse(&result)
    }
}

/// One stream-json user message: the page images (in order), then the request text.
fn user_message_line(req: &ProviderRequest) -> String {
    let mut content: Vec<Value> = req
        .images
        .iter()
        .map(|im| json!({"type": "image", "source": {"type": "base64", "media_type": im.mime, "data": b64(&im.data)}}))
        .collect();
    let mut text = req.user.clone();
    if req.thinking_budget.is_some() {
        // The budget is a soft cap: about one call in four still thought for ~11k tokens.
        text.push_str("\n\nKeep your reasoning brief: a short plan is enough, then answer.");
    }
    content.push(json!({"type": "text", "text": text}));
    let mut line = json!({"type": "user", "message": {"role": "user", "content": content}}).to_string();
    line.push('\n');
    line
}

/// Short description of one stream-json event, for the timing log.
fn event_label(v: &Value) -> String {
    match v["type"].as_str().unwrap_or("?") {
        "assistant" => {
            let blocks = v["message"]["content"].as_array().map(|b| {
                b.iter()
                    .map(|b| match b["type"].as_str() {
                        Some("tool_use") => format!("tool_use:{}", b["name"].as_str().unwrap_or("")),
                        Some(t) => t.to_string(),
                        None => "?".into(),
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            });
            format!("assistant[{}]", blocks.unwrap_or_default())
        }
        "result" => format!(
            "result model={:?} turns={} api_ms={} out={} thinking={} cost={}",
            v["modelUsage"].as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
            v["num_turns"], v["duration_api_ms"], v["usage"]["output_tokens"], v["usage"]["output_tokens_details"]["thinking_tokens"], v["total_cost_usd"]
        ),
        "system" => format!("system:{}", v["subtype"].as_str().unwrap_or("")),
        t => t.to_string(),
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
    fn resolves_npm_shims() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let shim = d.path().join("claude.cmd");
        // the shim npm writes on Windows
        let text = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\
                    \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";
        std::fs::write(&shim, text).unwrap();
        let same = |a: &Path, b: &Path| assert_eq!(a.canonicalize().unwrap(), b.canonicalize().unwrap());
        same(&resolve_shim(&shim).unwrap(), &exe);
        same(&find_claude(shim.to_str().unwrap()).unwrap(), &exe);
        assert_eq!(launcher(&exe).unwrap(), (exe.clone(), vec![]));

        // older shims start node with the JS entry
        let js = d.path().join("node_modules/@anthropic-ai/claude-code/cli.js");
        std::fs::write(&js, b"").unwrap();
        let old = "\"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*";
        same(&shim_target(old, d.path()).unwrap(), &js);
        assert!(resolve_shim(&js).is_none(), "not a shim");
        assert!(launcher(&shim).is_err(), "batch files are never started directly");
    }

    #[test]
    fn images_go_inline_before_the_text() {
        let req = ProviderRequest {
            system: "sys".into(),
            user: "translate".into(),
            images: vec![ImagePart { mime: "image/jpeg".into(), data: vec![1, 2, 3] }, ImagePart { mime: "image/png".into(), data: vec![4] }],
            schema: json!({}),
            max_output_tokens: 100,
            effort: "medium".into(),
            thinking_budget: None,
        };
        let line = user_message_line(&req);
        assert!(line.ends_with('\n') && !line.trim_end().contains('\n'), "one JSON line");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        let c = &v["message"]["content"];
        assert_eq!(c[0]["source"]["data"], "AQID");
        assert_eq!(c[1]["source"]["media_type"], "image/png");
        assert_eq!(c[2]["text"], "translate");
        assert!(event_label(&json!({"type": "result", "num_turns": 2})).starts_with("result"));
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
