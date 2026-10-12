//! Import folders/archives into a library: cargo run --example import -- <data-dir> <path>...
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = mangaai_lib::assets::Paths::new(std::path::PathBuf::from(&args[0]))?;
    let db = mangaai_lib::db::Db::open(&paths.db_path()).map_err(|e| anyhow::anyhow!(e.message))?;
    for p in &args[1..] {
        match mangaai_lib::library::import::import_path(&db, std::path::Path::new(p)) {
            Ok(r) => println!("{}: {} chapters, {} pages, {} warnings", r.title, r.chapters_added, r.pages_added, r.warnings.len()),
            Err(e) => println!("{p}: {}", e.message),
        }
    }
    Ok(())
}
