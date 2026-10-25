//! Live new-chapter check against a running Suwayomi, on a copy of a library database.
//! Reports only: nothing is queued.
//!
//! Usage: cargo run --example check_new -- <library.db copy> [http://127.0.0.1:4567]

use std::sync::Arc;

use mangaai_lib::db::Db;
use mangaai_lib::follow;
use mangaai_lib::sources::{SourceAdapter, Suwayomi};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = Db::open(std::path::Path::new(args.get(1).expect("path to a copy of library.db")))?;
    let url = args.get(2).cloned().unwrap_or_else(|| "http://127.0.0.1:4567".into());
    let e = |e: mangaai_lib::error::AppError| anyhow::anyhow!("{}: {}", e.code, e.message);
    let adapter: Arc<dyn SourceAdapter> = Arc::new(Suwayomi::new(&url, None).map_err(e)?);
    let t0 = std::time::Instant::now();
    let report = follow::check(&db, adapter, false).await.map_err(e)?;
    println!("checked {} series in {:.1}s, failed: {:?}", report.checked, t0.elapsed().as_secs_f32(), report.failed);
    for n in &report.new {
        println!("  {}: {:?}", n.title, n.chapters);
    }
    Ok(())
}
