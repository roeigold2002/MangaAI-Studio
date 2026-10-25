//! Sync and backup on a copy of a real library: copy → folder → empty second library, then a backup.
//!
//! Usage: cargo run --example sync_live -- <copy of library.db> <empty work folder>

use mangaai_lib::db::Db;
use mangaai_lib::sync;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let e = |e: mangaai_lib::error::AppError| anyhow::anyhow!("{}: {}", e.code, e.message);
    let a = Db::open(std::path::Path::new(&args[1]))?;
    let work = std::path::PathBuf::from(&args[2]);
    let folder = work.join("shared");
    std::fs::create_dir_all(&folder)?;

    let state = sync::export_state(&a).map_err(e)?;
    let chapters: usize = state.manga.iter().map(|m| m.chapters.len()).sum();
    println!("state: {} series, {} chapters with progress, {} glossary, {} memory", state.manga.len(), chapters,
        state.manga.iter().map(|m| m.glossary.len()).sum::<usize>(), state.manga.iter().map(|m| m.memory.len()).sum::<usize>());
    println!("computer A: {:?}", sync::sync_folder(&a, &folder).map_err(e)?);
    println!("state file: {} KB", std::fs::metadata(folder.join(sync::STATE_FILE))?.len() / 1024);

    let b = Db::open(&work.join("b.db"))?;
    println!("computer B (empty): {:?}", sync::sync_folder(&b, &folder).map_err(e)?);

    let t0 = std::time::Instant::now();
    let rep = sync::create_backup(&a, &work, &work.join("backup.zip"), false).map_err(e)?;
    println!("backup: {} KB, {} files, {:.1}s", rep.bytes / 1024, rep.files, t0.elapsed().as_secs_f32());
    Ok(())
}
