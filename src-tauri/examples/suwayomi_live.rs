//! Live check against a running Suwayomi-Server (no mocks):
//! ping → list sources → browse → manga + chapters → download through the
//! app's download queue → chapter readable from the local library.
//!
//! Usage: cargo run --example suwayomi_live -- [http://127.0.0.1:4567] [source-name-filter]

use std::sync::Arc;

use mangaai_lib::assets::Paths;
use mangaai_lib::db::Db;
use mangaai_lib::downloads::{self, DownloadRunner};
use mangaai_lib::library::queries;
use mangaai_lib::pipeline::NoEvents;
use mangaai_lib::protocol;
use mangaai_lib::sources::{SourceAdapter, Suwayomi};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let base = args.get(1).cloned().unwrap_or_else(|| "http://127.0.0.1:4567".into());
    let filter = args.get(2).cloned().unwrap_or_else(|| "local".into()).to_lowercase();
    let e = |e: mangaai_lib::error::AppError| anyhow::anyhow!("{}: {}", e.code, e.message);

    let s = Suwayomi::new(&base, None).map_err(e)?;
    println!("server: {}", s.ping().await.map_err(e)?);
    let sources = s.sources().await.map_err(e)?;
    println!("sources: {:?}", sources.iter().map(|x| format!("{} [{}] {}", x.name, x.lang, x.id)).collect::<Vec<_>>());
    let src = sources.iter().find(|x| x.name.to_lowercase().contains(&filter)).ok_or_else(|| anyhow::anyhow!("no source matching {filter}"))?;
    let page = s.browse(&src.id, "popular", None, 1).await.map_err(e)?;
    println!("browse popular: {} result(s), next page: {}", page.mangas.len(), page.has_next_page);
    let first = page.mangas.first().ok_or_else(|| anyhow::anyhow!("source returned no manga"))?;
    let search = s.browse(&src.id, "search", Some(&first.title[..first.title.len().min(6)]), 1).await.map_err(e)?;
    println!("search '{}': {} result(s)", &first.title[..first.title.len().min(6)], search.mangas.len());
    let manga = s.manga(&first.manga_ref).await.map_err(e)?;
    let chapters = s.chapters(&manga.manga_ref).await.map_err(e)?;
    println!("manga: {} ({} chapters): {:?}", manga.title, chapters.len(), chapters.iter().map(|c| &c.title).collect::<Vec<_>>());

    // Download through the app's queue into a fresh library.
    let tmp = tempfile::tempdir()?;
    let paths = Paths::new(tmp.path().join("app"))?;
    let db = Db::open(&paths.db_path()).map_err(e)?;
    let base2 = base.clone();
    let runner = DownloadRunner {
        db: db.clone(),
        paths: paths.clone(),
        events: Arc::new(NoEvents),
        notify: Arc::new(tokio::sync::Notify::new()),
        adapter: Arc::new(move || Ok(Arc::new(Suwayomi::new(&base2, None)?) as Arc<dyn SourceAdapter>)),
    };
    let (added, skipped) = downloads::enqueue(&db, &manga, &chapters[..1]).map_err(e)?;
    println!("enqueued: added={added} skipped={skipped}");
    let dl = downloads::list(&db).map_err(e)?[0].clone();
    // cancel then retry, then run for real
    downloads::cancel(&db, dl.id).map_err(e)?;
    println!("after cancel: {}", downloads::get(&db, dl.id).map_err(e)?.status);
    downloads::retry(&db, dl.id).map_err(e)?;
    let t0 = std::time::Instant::now();
    runner.run_one(dl.id).await.map_err(e)?;
    let dl = downloads::get(&db, dl.id).map_err(e)?;
    println!("download: status={} {}/{} pages ({:.1}s)", dl.status, dl.progress, dl.total, t0.elapsed().as_secs_f32());
    assert_eq!(dl.status, "done");
    let (again, skip2) = downloads::enqueue(&db, &manga, &chapters[..1]).map_err(e)?;
    println!("re-enqueue same chapter: added={again} skipped={skip2}");
    assert_eq!(again, 0);

    let view = queries::chapter_view(&db, dl.chapter_id.unwrap()).map_err(e)?;
    println!("library chapter: '{}' {} pages, downloaded={}", view.chapter.title, view.pages.len(), view.chapter.downloaded);
    let r = protocol::handle(&db, &paths, &format!("/page/{}", view.pages[0].id));
    println!("first page served from disk: HTTP {} {} {} bytes", r.status, r.mime, r.body.len());
    assert_eq!(r.status, 200);
    Ok(())
}
