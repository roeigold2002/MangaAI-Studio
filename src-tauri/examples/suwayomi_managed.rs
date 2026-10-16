//! Live check of the app-managed Suwayomi-Server on this OS (no mocks): download the pinned
//! release (checksum verified), unpack, start it on 127.0.0.1, wait until GraphQL answers,
//! list sources, then stop it. This is what "התקנה והפעלה" does in the app.
//!
//! Usage: cargo run --example suwayomi_managed -- [port]
//! Installs into the app's data folder (MANGAAI_DATA_DIR overrides), so the app reuses it.

use std::time::{Duration, Instant};

use mangaai_lib::runtime::Runtime;
use mangaai_lib::sources::{SourceAdapter, Suwayomi};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(4568);
    let e = |e: mangaai_lib::error::AppError| anyhow::anyhow!("{}: {}", e.code, e.message);
    let root = std::env::var_os("MANGAAI_DATA_DIR").map(std::path::PathBuf::from).unwrap_or_else(mangaai_lib::platform::default_data_dir);
    std::fs::create_dir_all(root.join("logs"))?;
    let rt = Runtime::new(&root, root.join("logs").join("suwayomi.log"));
    println!("status: {:?}", rt.status().await);

    if !rt.is_installed() {
        let t0 = Instant::now();
        rt.install(|done, total| println!("  {} / {:?} MB", done / 1_000_000, total.map(|t| t / 1_000_000))).await.map_err(e)?;
        println!("installed in {:.0}s", t0.elapsed().as_secs_f32());
    }

    rt.start(port).await.map_err(e)?;
    let url = format!("http://127.0.0.1:{port}");
    let sw = Suwayomi::new(&url, None).map_err(e)?;
    let t0 = Instant::now();
    let version = loop {
        if let Some(msg) = rt.exited().await {
            anyhow::bail!("{msg}");
        }
        match sw.ping().await {
            Ok(v) => break v,
            Err(_) if t0.elapsed() < Duration::from_secs(180) => tokio::time::sleep(Duration::from_secs(2)).await,
            Err(err) => anyhow::bail!("server did not answer: {}", err.message),
        }
    };
    println!("server up after {:.0}s: {version}", t0.elapsed().as_secs_f32());
    let sources = sw.sources().await.map_err(e)?;
    println!("sources: {:?}", sources.iter().map(|s| format!("{} [{}]", s.name, s.lang)).collect::<Vec<_>>());

    rt.stop().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let down = sw.ping().await.is_err();
    println!("stopped: {down}");
    anyhow::ensure!(down, "server still answering after stop");
    Ok(())
}
