//! Managed Suwayomi-Server: one-click install of the pinned official release
//! (checksum verified), started by the app on 127.0.0.1 with its own data
//! folder, stopped when the app quits. A server the user runs themselves is
//! used as-is and never touched.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::{AppError, AppResult};

pub const VERSION: &str = "v2.4.2366";
const RELEASE_BASE: &str = "https://github.com/Suwayomi/Suwayomi-Server/releases/download";

/// Release asset for this platform, if Suwayomi publishes one.
pub fn asset_name() -> Option<String> {
    let plat = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "macOS-arm64.tar.gz",
        ("macos", "x86_64") => "macOS-x64.tar.gz",
        ("linux", "x86_64") => "linux-x64.tar.gz",
        ("windows", "x86_64") => "windows-x64.zip",
        _ => return None,
    };
    Some(format!("Suwayomi-Server-{VERSION}-{plat}"))
}

/// Pick the checksum for `asset` out of the release's Checksums.sha256 file.
pub fn checksum_for(checksums: &str, asset: &str) -> Option<String> {
    checksums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let sum = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset && sum.len() == 64).then(|| sum.to_lowercase())
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub supported: bool,
    pub installed: bool,
    pub running: bool,
    pub version: &'static str,
    pub install_dir: String,
    pub download_size_mb: Option<u64>,
}

pub struct Runtime {
    root: PathBuf,
    child: Mutex<Option<tokio::process::Child>>,
    log_path: PathBuf,
}

impl Runtime {
    pub fn new(app_data: &Path, log_path: PathBuf) -> Self {
        Self { root: app_data.join("suwayomi"), child: Mutex::new(None), log_path }
    }

    fn install_dir(&self) -> PathBuf {
        self.root.join(VERSION)
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// The folder Suwayomi's built-in "Local source" reads.
    pub fn local_dir(&self) -> PathBuf {
        self.data_dir().join("local")
    }

    fn marker(&self) -> PathBuf {
        self.install_dir().join(".installed")
    }

    pub fn is_installed(&self) -> bool {
        self.marker().exists() && self.java().is_some()
    }

    fn java(&self) -> Option<PathBuf> {
        let exe = if cfg!(windows) { "java.exe" } else { "java" };
        walkdir::WalkDir::new(self.install_dir())
            .max_depth(6)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name() == exe && e.path().parent().map(|p| p.ends_with("bin")).unwrap_or(false))
            .map(|e| e.into_path())
    }

    fn server_jar(&self) -> Option<PathBuf> {
        walkdir::WalkDir::new(self.install_dir())
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name() == "Suwayomi-Server.jar")
            .map(|e| e.into_path())
    }

    pub async fn status(&self) -> RuntimeStatus {
        let running = {
            let mut g = self.child.lock().await;
            match g.as_mut() {
                Some(c) => c.try_wait().ok().flatten().is_none(),
                None => false,
            }
        };
        RuntimeStatus {
            supported: asset_name().is_some(),
            installed: self.is_installed(),
            running,
            version: VERSION,
            install_dir: self.install_dir().to_string_lossy().into_owned(),
            download_size_mb: Some(330),
        }
    }

    /// Download, verify and unpack the official release. `progress(done, total)` in bytes.
    pub async fn install(&self, progress: impl Fn(u64, Option<u64>)) -> AppResult<()> {
        let asset = asset_name().ok_or_else(|| AppError::new("unsupported", "Suwayomi has no release for this platform"))?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| AppError::new("network", e.to_string()))?;
        let sums = client
            .get(format!("{RELEASE_BASE}/{VERSION}/Checksums.sha256"))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AppError::new("network", format!("could not download the checksum list: {e}")))?
            .text()
            .await
            .map_err(|e| AppError::new("network", e.to_string()))?;
        let expected = checksum_for(&sums, &asset).ok_or_else(|| AppError::new("checksum", "the release has no checksum for this file"))?;

        std::fs::create_dir_all(&self.root)?;
        crate::assets::ensure_space(&self.root, 900 * 1024 * 1024)?;
        let part = self.root.join(format!("{asset}.part"));
        let mut resp = client
            .get(format!("{RELEASE_BASE}/{VERSION}/{asset}"))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AppError::new("network", format!("download failed: {e}")))?;
        let total = resp.content_length();
        let mut file = tokio::fs::File::create(&part).await?;
        let mut hasher = Sha256::new();
        let mut done = 0u64;
        let mut last = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(|e| AppError::new("network", format!("download interrupted: {e}")))? {
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            if done - last > 2 * 1024 * 1024 {
                last = done;
                progress(done, total);
            }
        }
        file.flush().await?;
        drop(file);
        let got = hex::encode(hasher.finalize());
        if got != expected {
            let _ = std::fs::remove_file(&part);
            return Err(AppError::new("checksum", "the downloaded file does not match the published checksum; nothing was installed"));
        }
        progress(done, total);

        let dir = self.install_dir();
        let staging = self.root.join(format!("{VERSION}.staging"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        if asset.ends_with(".zip") {
            let f = std::fs::File::open(&part)?;
            zip::ZipArchive::new(f)?.extract(&staging)?;
        } else {
            let st = tokio::process::Command::new("tar").arg("-xzf").arg(&part).arg("-C").arg(&staging).status().await?;
            if !st.success() {
                return Err(AppError::new("extract", "could not unpack the Suwayomi archive"));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&staging, &dir)?;
        let _ = std::fs::remove_file(&part);
        if self.java().is_none() || self.server_jar().is_none() {
            return Err(AppError::new("extract", "the archive did not contain the expected server files"));
        }
        std::fs::write(self.marker(), VERSION)?;
        std::fs::create_dir_all(self.local_dir())?;
        Ok(())
    }

    /// Start the managed server (no-op if it is already running).
    pub async fn start(&self, port: u16) -> AppResult<()> {
        let mut g = self.child.lock().await;
        if let Some(c) = g.as_mut() {
            if c.try_wait().ok().flatten().is_none() {
                return Ok(());
            }
        }
        let java = self.java().ok_or_else(|| AppError::new("not_installed", "Suwayomi is not installed"))?;
        let jar = self.server_jar().ok_or_else(|| AppError::new("not_installed", "Suwayomi is not installed"))?;
        std::fs::create_dir_all(self.local_dir())?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log_path)?;
        let prop = |k: &str, v: &str| format!("-Dsuwayomi.tachidesk.config.server.{k}={v}");
        let mut cmd = tokio::process::Command::new(java);
        cmd.arg(prop("rootDir", &self.data_dir().to_string_lossy()))
            .arg(prop("ip", "127.0.0.1"))
            .arg(prop("port", &port.to_string()))
            .arg(prop("initialOpenInBrowserEnabled", "false"))
            .arg(prop("systemTrayEnabled", "false"))
            .arg(prop("webUIEnabled", "false"))
            .arg("-Djava.awt.headless=true")
            .arg("-jar")
            .arg(jar)
            .current_dir(self.install_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        let child = cmd.spawn().map_err(|e| AppError::new("start_failed", format!("could not start Suwayomi: {e}")))?;
        *g = Some(child);
        Ok(())
    }

    pub async fn stop(&self) {
        let mut g = self.child.lock().await;
        if let Some(mut c) = g.take() {
            let _ = c.kill().await;
        }
    }

    /// True if the managed process died (e.g. port already in use).
    pub async fn exited(&self) -> Option<String> {
        let mut g = self.child.lock().await;
        let c = g.as_mut()?;
        match c.try_wait() {
            Ok(Some(st)) => Some(format!("Suwayomi exited ({st}); see logs/suwayomi.log")),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_checksum_for_asset() {
        let sums = "aaa  other.zip\n3b269855928070253a4d9f7c4a57d9ff99d0272102410233c459b02a732e87e7  Suwayomi-Server-v2.4.2366-macOS-arm64.tar.gz\n";
        assert_eq!(
            checksum_for(sums, "Suwayomi-Server-v2.4.2366-macOS-arm64.tar.gz").as_deref(),
            Some("3b269855928070253a4d9f7c4a57d9ff99d0272102410233c459b02a732e87e7")
        );
        assert_eq!(checksum_for(sums, "missing"), None);
        assert_eq!(checksum_for(sums, "other.zip"), None, "malformed sums are rejected");
    }

    #[test]
    fn this_platform_has_an_asset() {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(asset_name().unwrap(), "Suwayomi-Server-v2.4.2366-macOS-arm64.tar.gz");
        }
    }

    #[tokio::test]
    async fn not_installed_status() {
        let d = tempfile::tempdir().unwrap();
        let r = Runtime::new(d.path(), d.path().join("s.log"));
        let s = r.status().await;
        assert!(!s.installed && !s.running);
        assert_eq!(r.start(4999).await.unwrap_err().code, "not_installed");
    }
}
