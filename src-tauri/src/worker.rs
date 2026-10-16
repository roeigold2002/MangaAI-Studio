//! Bridge to the Python vision worker (JSON lines over stdio).
//! `Vision` is the interface the pipeline uses, so tests can substitute a fake.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetectedRegion {
    pub rid: String,
    pub kind: String,
    pub bbox: [i64; 4],
    #[serde(default)]
    pub parts: Vec<[i64; 4]>,
    pub bubble: Option<[i64; 4]>,
    pub vertical: bool,
    pub text: String,
    pub confidence: Option<f64>,
    #[serde(default)]
    pub punct_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub version: String,
    pub width: i64,
    pub height: i64,
    pub regions: Vec<DetectedRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageResult {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub notes: Vec<Value>,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub placed: Vec<Value>,
}

#[async_trait]
pub trait Vision: Send + Sync {
    async fn health(&self) -> AppResult<Value>;
    /// `ocr`: language of the local OCR to run ("ja", "en"), or None for detection only.
    async fn analyze(&self, image: &Path, rtl: bool, threshold: f32, device: &str, ocr: Option<&str>) -> AppResult<Analysis>;
    async fn ocr_region(&self, image: &Path, bbox: [i64; 4], device: &str, lang: &str) -> AppResult<(String, Option<f64>)>;
    async fn clean(&self, image: &Path, out: &Path, regions: Value, method: &str, device: &str) -> AppResult<StageResult>;
    async fn render(&self, clean: &Path, out: &Path, regions: Value, settings: Value) -> AppResult<StageResult>;
    async fn prepare_models(&self) -> AppResult<Value>;
    /// Version strings that are part of cache keys.
    fn versions(&self) -> (String, String, String) {
        ("a2".into(), "i3".into(), "r2".into())
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub python: PathBuf,
    pub worker_dir: PathBuf,
    pub models_dir: PathBuf,
    pub fonts_dir: PathBuf,
    pub log_path: PathBuf,
    /// Folder with native libraries the worker loads at run time (fribidi-0.dll on Windows).
    pub native_dir: Option<PathBuf>,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct PythonWorker {
    cfg: WorkerConfig,
    proc: Mutex<Option<Proc>>,
    next_id: AtomicU64,
}

impl PythonWorker {
    pub fn new(cfg: WorkerConfig) -> Self {
        Self { cfg, proc: Mutex::new(None), next_id: AtomicU64::new(1) }
    }

    pub fn config(&self) -> &WorkerConfig {
        &self.cfg
    }

    fn spawn(&self, allow_network: bool) -> AppResult<Proc> {
        if !self.cfg.python.exists() {
            return Err(AppError::new(
                "engine_missing",
                "The AI engine (Python worker) is not installed. Open Settings → AI engine → Install.",
            ));
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.cfg.log_path)?;
        let mut cmd = tokio::process::Command::new(&self.cfg.python);
        crate::platform::no_window(&mut cmd);
        cmd.arg("-m")
            .arg("mangaai_worker")
            .current_dir(&self.cfg.worker_dir)
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONPATH", &self.cfg.worker_dir)
            .env("MANGAAI_MODELS_DIR", &self.cfg.models_dir)
            .env("TOKENIZERS_PARALLELISM", "false")
            .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
            // The protocol is UTF-8 on every platform (Windows defaults to the ANSI code page).
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            // Windows without Developer Mode has no symlinks; the cache works, the warning is noise.
            .env("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        if cfg!(target_os = "macos") {
            // Pillow loads libraqm/fribidi dynamically; GUI apps don't see Homebrew's lib dir.
            cmd.env("DYLD_FALLBACK_LIBRARY_PATH", "/opt/homebrew/lib:/usr/local/lib:/usr/lib");
        }
        if let Some(d) = &self.cfg.native_dir {
            cmd.env("MANGAAI_NATIVE_DIR", d);
        }
        if !allow_network {
            // No hidden downloads: models load from the local cache only.
            cmd.env("HF_HUB_OFFLINE", "1").env("TRANSFORMERS_OFFLINE", "1");
        }
        let mut child = cmd.spawn().map_err(|e| AppError::new("engine_start", format!("could not start the AI engine: {e}")))?;
        crate::platform::adopt(&child);
        let stdin = child.stdin.take().ok_or_else(|| AppError::new("engine_start", "no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| AppError::new("engine_start", "no stdout"))?);
        Ok(Proc { child, stdin, stdout })
    }

    async fn call(&self, method: &str, params: Value, timeout: Duration) -> AppResult<Value> {
        self.call_with(method, params, timeout, false).await
    }

    async fn call_with(&self, method: &str, params: Value, timeout: Duration, allow_network: bool) -> AppResult<Value> {
        let mut guard = self.proc.lock().await;
        if allow_network {
            // run this request in a fresh process that may download
            if let Some(mut p) = guard.take() {
                let _ = p.child.kill().await;
            }
        }
        if let Some(p) = guard.as_mut() {
            if p.child.try_wait().ok().flatten().is_some() {
                *guard = None; // crashed; restart below
            }
        }
        if guard.is_none() {
            *guard = Some(self.spawn(allow_network)?);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let line = serde_json::to_string(&json!({"id": id, "method": method, "params": params}))? + "\n";
        let p = guard.as_mut().expect("worker running");
        let io = async {
            p.stdin.write_all(line.as_bytes()).await?;
            p.stdin.flush().await?;
            loop {
                let mut buf = String::new();
                let n = p.stdout.read_line(&mut buf).await?;
                if n == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "worker exited"));
                }
                let Ok(v) = serde_json::from_str::<Value>(&buf) else { continue };
                if v["id"].as_u64() == Some(id) {
                    return Ok(v);
                }
            }
        };
        let res = tokio::time::timeout(timeout, io).await;
        let v = match res {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                *guard = None;
                return Err(AppError::new(
                    "engine_crashed",
                    format!("the AI engine stopped unexpectedly ({e}); see logs/worker.log"),
                ));
            }
            Err(_) => {
                if let Some(mut p) = guard.take() {
                    let _ = p.child.kill().await;
                }
                return Err(AppError::new("engine_timeout", format!("the AI engine did not finish '{method}' in time")));
            }
        };
        if allow_network {
            // don't keep a network-enabled process around
            if let Some(mut p) = guard.take() {
                let _ = p.child.kill().await;
            }
        }
        if v["ok"] == true {
            Ok(v["result"].clone())
        } else {
            let code = v["error"]["code"].as_str().unwrap_or("worker_error").to_string();
            let msg = v["error"]["message"].as_str().unwrap_or("unknown error").to_string();
            Err(AppError { code, message: msg })
        }
    }

    pub async fn shutdown(&self) {
        let mut guard = self.proc.lock().await;
        if let Some(mut p) = guard.take() {
            let _ = p.stdin.write_all(b"{\"id\":0,\"method\":\"shutdown\"}\n").await;
            let _ = tokio::time::timeout(Duration::from_secs(2), p.child.wait()).await;
            let _ = p.child.kill().await;
        }
    }
}

#[async_trait]
impl Vision for PythonWorker {
    async fn health(&self) -> AppResult<Value> {
        self.call("health", json!({}), Duration::from_secs(60)).await
    }

    async fn analyze(&self, image: &Path, rtl: bool, threshold: f32, device: &str, ocr: Option<&str>) -> AppResult<Analysis> {
        let v = self
            .call(
                "analyze",
                json!({"image": image, "rtl": rtl, "threshold": threshold, "device": device, "ocr": ocr.is_some(), "lang": ocr.unwrap_or("ja")}),
                Duration::from_secs(600),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    async fn ocr_region(&self, image: &Path, bbox: [i64; 4], device: &str, lang: &str) -> AppResult<(String, Option<f64>)> {
        let v = self.call("ocr_region", json!({"image": image, "box": bbox, "device": device, "lang": lang}), Duration::from_secs(120)).await?;
        Ok((v["text"].as_str().unwrap_or_default().to_string(), v["confidence"].as_f64()))
    }

    async fn clean(&self, image: &Path, out: &Path, regions: Value, method: &str, device: &str) -> AppResult<StageResult> {
        let v = self
            .call(
                "clean",
                json!({"image": image, "out": out, "regions": regions, "method": method, "device": device}),
                Duration::from_secs(600),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    async fn render(&self, clean: &Path, out: &Path, regions: Value, settings: Value) -> AppResult<StageResult> {
        let v = self
            .call(
                "render",
                json!({"clean": clean, "out": out, "regions": regions, "fonts_dir": self.cfg.fonts_dir, "settings": settings}),
                Duration::from_secs(300),
            )
            .await?;
        Ok(serde_json::from_value(v)?)
    }

    async fn prepare_models(&self) -> AppResult<Value> {
        self.call_with("prepare_models", json!({}), Duration::from_secs(3600), true).await
    }
}

/// Locate the worker sources and its Python interpreter.
pub fn resolve(app_data: &Path, resource_dir: Option<&Path>) -> (PathBuf, PathBuf) {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../worker");
    let worker_dir = std::env::var_os("MANGAAI_WORKER_DIR")
        .map(PathBuf::from)
        .filter(|p| p.join("mangaai_worker").exists())
        .or_else(|| dev.join("mangaai_worker").exists().then(|| dev.clone()))
        .or_else(|| resource_dir.map(|r| r.join("worker")).filter(|p| p.join("mangaai_worker").exists()))
        .unwrap_or(dev);
    let venv_bin = if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" };
    let python = [app_data.join("worker-venv").join(venv_bin), worker_dir.join(".venv").join(venv_bin)]
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| app_data.join("worker-venv").join(venv_bin));
    (crate::platform::canonicalize(&worker_dir).unwrap_or(worker_dir), python)
}

/// Folder holding fribidi-0.dll (Windows only): bundled resources, else the dev build output.
pub fn resolve_native_dir(resource_dir: Option<&Path>) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows").join("native");
    resource_dir
        .map(|r| r.join("native"))
        .into_iter()
        .chain(std::iter::once(dev))
        .find(|d| d.join("fribidi-0.dll").exists())
}

pub fn find_uv() -> Option<PathBuf> {
    which::which("uv").ok().or_else(|| {
        let home = crate::platform::home_dir();
        let uv = crate::platform::exe("uv");
        let mut candidates = vec![home.join(".local/bin").join(&uv), home.join(".cargo/bin").join(&uv)];
        if cfg!(windows) {
            if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                candidates.push(local.join("Microsoft/WinGet/Links/uv.exe"));
                candidates.push(local.join("Programs/uv/uv.exe"));
            }
        } else {
            candidates.extend([PathBuf::from("/opt/homebrew/bin/uv"), PathBuf::from("/usr/local/bin/uv")]);
        }
        candidates.into_iter().find(|p| p.exists())
    })
}

#[cfg(test)]
pub mod fake {
    //! Deterministic stand-in for the Python worker.
    use super::*;
    use parking_lot::Mutex as PMutex;

    pub struct FakeVision {
        pub regions: Vec<DetectedRegion>,
        pub calls: PMutex<Vec<String>>,
        pub fail_render: PMutex<bool>,
    }

    impl FakeVision {
        pub fn new(texts: &[&str]) -> Self {
            let regions = texts
                .iter()
                .enumerate()
                .map(|(i, t)| DetectedRegion {
                    rid: format!("r{}", i + 1),
                    kind: "bubble".into(),
                    bbox: [10, 10 + i as i64 * 20, 50, 25 + i as i64 * 20],
                    parts: vec![],
                    bubble: Some([0, 0, 60, 60]),
                    vertical: false,
                    text: t.to_string(),
                    confidence: Some(0.95),
                    punct_only: false,
                })
                .collect();
            Self { regions, calls: PMutex::new(vec![]), fail_render: PMutex::new(false) }
        }
    }

    #[async_trait]
    impl Vision for FakeVision {
        async fn health(&self) -> AppResult<Value> {
            Ok(json!({"raqm": true}))
        }
        async fn analyze(&self, _image: &Path, _rtl: bool, _t: f32, _d: &str, ocr: Option<&str>) -> AppResult<Analysis> {
            self.calls.lock().push(format!("analyze:ocr={}", ocr.unwrap_or("false")));
            let mut regions = self.regions.clone();
            if ocr.is_none() {
                regions.iter_mut().for_each(|r| { r.text.clear(); r.confidence = None; });
            }
            Ok(Analysis { version: "a2".into(), width: 100, height: 100, regions })
        }
        async fn ocr_region(&self, _image: &Path, _b: [i64; 4], _d: &str, lang: &str) -> AppResult<(String, Option<f64>)> {
            self.calls.lock().push(format!("ocr_region:{lang}"));
            Ok(("再OCR".into(), Some(0.9)))
        }
        async fn clean(&self, image: &Path, out: &Path, _r: Value, _m: &str, _d: &str) -> AppResult<StageResult> {
            self.calls.lock().push("clean".into());
            std::fs::create_dir_all(out.parent().unwrap())?;
            std::fs::copy(image, out)?;
            Ok(StageResult::default())
        }
        async fn render(&self, clean: &Path, out: &Path, regions: Value, _s: Value) -> AppResult<StageResult> {
            self.calls.lock().push(format!("render:{}", regions.as_array().map(|a| a.len()).unwrap_or(0)));
            if *self.fail_render.lock() {
                return Err(AppError::new("render_failed", "fake render failure"));
            }
            std::fs::copy(clean, out)?;
            Ok(StageResult::default())
        }
        async fn prepare_models(&self) -> AppResult<Value> {
            Ok(json!({}))
        }
    }
}
