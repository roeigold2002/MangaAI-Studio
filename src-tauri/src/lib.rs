pub mod assets;
pub mod catalog;
pub mod chapter_pick;
pub mod commands;
#[cfg(all(target_os = "macos", feature = "dev-snapshot"))]
mod devsnap;
pub mod db;
pub mod downloads;
pub mod editor;
pub mod error;
pub mod jobs;
pub mod library;
pub mod pipeline;
pub mod platform;
pub mod protocol;
pub mod providers;
pub mod runtime;
pub mod secrets;
pub mod settings;
pub mod sources;
pub mod translate;
pub mod worker;

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Notify;

use crate::assets::Paths;
use crate::commands::AppState;
use crate::db::Db;
use crate::pipeline::{Engine, EventSink};
use crate::secrets::KeyringStore;
use crate::worker::{PythonWorker, WorkerConfig};

struct TauriEvents(tauri::AppHandle);

impl EventSink for TauriEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = self.0.emit(event, payload);
    }
}

fn init_logging(paths: &Paths) -> tracing_appender::non_blocking::WorkerGuard {
    let appender = tracing_appender::rolling::daily(paths.logs_dir(), "app.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,mangaai_lib=info".into()))
        .with_writer(writer)
        .with_ansi(false)
        .json()
        .try_init();
    guard
}

fn remote_image(st: &AppState, hex_url: &str) -> protocol::Reply {
    let Ok(bytes) = hex::decode(hex_url.trim_matches('/')) else { return protocol::Reply::error(400, "bad url") };
    let url = String::from_utf8_lossy(&bytes).into_owned();
    let adapter = match commands::make_adapter(&st.db, st.secrets.as_ref()) {
        Ok(a) => a,
        Err(e) => return protocol::Reply::error(503, &e.message),
    };
    match tauri::async_runtime::block_on(adapter.fetch(&url)) {
        Ok((body, ct)) => protocol::Reply {
            status: 200,
            mime: ct.unwrap_or_else(|| "image/jpeg".into()),
            body,
            cache: "max-age=3600",
        },
        Err(e) => protocol::Reply::error(502, &e.message),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // in-app updates: the UI asks before downloading, installing and restarting
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .register_asynchronous_uri_scheme_protocol("mangaimg", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8_lossy().into_owned();
            tauri::async_runtime::spawn_blocking(move || {
                let st = app.state::<AppState>();
                let r = if let Some(h) = path.trim_start_matches('/').strip_prefix("remote/") {
                    // thumbnails from the configured Suwayomi server only
                    remote_image(&st, h)
                } else {
                    protocol::handle(&st.db, &st.paths, &path)
                };
                let resp = tauri::http::Response::builder()
                    .status(r.status)
                    .header("Content-Type", r.mime)
                    .header("Cache-Control", r.cache)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(r.body)
                    .unwrap();
                responder.respond(resp);
            });
        })
        .setup(|app| {
            // MANGAAI_DATA_DIR lets the library live elsewhere (portable setups, testing).
            let root = match std::env::var_os("MANGAAI_DATA_DIR") {
                Some(d) => std::path::PathBuf::from(d),
                None => app.path().app_data_dir()?,
            };
            let paths = Paths::new(root.clone())?;
            let guard = init_logging(&paths);
            Box::leak(Box::new(guard));
            tracing::info!(version = env!("CARGO_PKG_VERSION"), data = %root.display(), "starting");

            let db = Db::open(&paths.db_path())?;
            let recovered_jobs = jobs::recover(&db)?;
            let recovered_dl = downloads::recover(&db)?;
            tracing::info!(recovered_jobs, recovered_dl, "startup recovery");

            let resource_dir = app.path().resource_dir().ok();
            let (worker_dir, python) = worker::resolve(&root, resource_dir.as_deref());
            let fonts_dir = {
                let dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/fonts");
                if dev.exists() {
                    platform::canonicalize(&dev).unwrap_or(dev)
                } else {
                    resource_dir.clone().map(|r| r.join("fonts")).unwrap_or(dev)
                }
            };
            let models_dir = root.join("models");
            std::fs::create_dir_all(&models_dir)?;
            let vision = Arc::new(PythonWorker::new(WorkerConfig {
                python,
                worker_dir,
                models_dir,
                fonts_dir,
                log_path: paths.logs_dir().join("worker.log"),
                native_dir: worker::resolve_native_dir(resource_dir.as_deref()),
            }));
            let events: Arc<dyn EventSink> = Arc::new(TauriEvents(app.handle().clone()));
            let engine = Engine::new(db.clone(), paths.clone(), vision.clone(), events.clone());
            let secrets: Arc<dyn secrets::SecretStore> = Arc::new(KeyringStore);
            let job_notify = Arc::new(Notify::new());
            let dl_notify = Arc::new(Notify::new());

            let runner = Arc::new(jobs::Runner {
                engine: engine.clone(),
                secrets: secrets.clone(),
                notify: job_notify.clone(),
                provider_override: None,
            });
            tauri::async_runtime::spawn(runner.run_forever());

            let concurrency = settings::load(&db).map(|s| s.sources.download_concurrency as usize).unwrap_or(2);
            let (adb, asec) = (db.clone(), secrets.clone());
            let dl = Arc::new(downloads::DownloadRunner {
                db: db.clone(),
                paths: paths.clone(),
                events,
                notify: dl_notify.clone(),
                adapter: Arc::new(move || commands::make_adapter(&adb, asec.as_ref())),
            });
            tauri::async_runtime::spawn(dl.run_forever(concurrency));

            let runtime = Arc::new(runtime::Runtime::new(&root, paths.logs_dir().join("suwayomi.log")));
            // Start the app-managed Suwayomi in the background (a user-run server is left alone).
            if let Ok(s) = settings::load(&db) {
                if s.sources.managed && s.sources.suwayomi_enabled && runtime.is_installed() {
                    let (r, url, port) = (runtime.clone(), s.sources.suwayomi_url.clone(), s.sources.managed_port);
                    let (handle, dl) = (app.handle().clone(), dl_notify.clone());
                    tauri::async_runtime::spawn(async move {
                        let Ok(sw) = sources::Suwayomi::new(&url, None) else { return };
                        if sources::SourceAdapter::ping(&sw).await.is_ok() {
                            return;
                        }
                        if let Err(e) = r.start(port).await {
                            tracing::warn!("managed Suwayomi not started: {}", e.message);
                            return;
                        }
                        // Pages that asked while the server was booting showed it as off.
                        for _ in 0..90 {
                            if sources::SourceAdapter::ping(&sw).await.is_ok() {
                                let _ = handle.emit("sources-changed", serde_json::json!({}));
                                dl.notify_one();
                                return;
                            }
                            if r.exited().await.is_some() {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                        let _ = handle.emit("sources-changed", serde_json::json!({}));
                    });
                }
            }
            #[cfg(unix)]
            {
                let (r, w, handle) = (runtime.clone(), vision.clone(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt())) else { return };
                    tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
                    w.shutdown().await;
                    r.stop().await;
                    handle.exit(0);
                });
            }
            #[cfg(windows)]
            {
                // Ctrl+C / closing the console of a dev build: stop children before exiting.
                let (r, w, handle) = (runtime.clone(), vision.clone(), app.handle().clone());
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::windows::{ctrl_c, ctrl_close};
                    let (Ok(mut c), Ok(mut close)) = (ctrl_c(), ctrl_close()) else { return };
                    tokio::select! { _ = c.recv() => {}, _ = close.recv() => {} }
                    w.shutdown().await;
                    r.stop().await;
                    handle.exit(0);
                });
            }
            let search_gen = Arc::new(std::sync::atomic::AtomicU64::new(0));
            app.manage(AppState { runtime, search_gen, db, paths, engine, worker: vision, secrets, job_notify, dl_notify, resource_dir });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let st = window.state::<AppState>();
                let (w, r) = (st.worker.clone(), st.runtime.clone());
                tauri::async_runtime::block_on(async move {
                    w.shutdown().await;
                    r.stop().await;
                });
            }
        })
        .invoke_handler(handlers())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Quit from the menu / Cmd+Q / last window: stop the worker and the managed server.
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<AppState>() {
                    let (w, r) = (st.worker.clone(), st.runtime.clone());
                    tauri::async_runtime::block_on(async move {
                        w.shutdown().await;
                        r.stop().await;
                    });
                }
            }
        });
}

macro_rules! app_handlers {
    ($($extra:path),*) => {
        tauri::generate_handler![$($extra,)*
            commands::list_manga,
            commands::manga_detail,
            commands::chapter_view,
            commands::import_paths,
            commands::rescan_manga,
            commands::update_manga,
            commands::remove_manga,
            commands::set_chapters_read,
            commands::save_progress,
            commands::reading_history,
            commands::clear_history,
            commands::list_categories,
            commands::create_category,
            commands::rename_category,
            commands::delete_category,
            commands::home_data,
            commands::get_settings,
            commands::save_settings,
            commands::set_api_key,
            commands::api_key_status,
            commands::test_provider,
            commands::list_ollama_models,
            commands::claude_cli_status,
            commands::engine_status,
            commands::install_engine,
            commands::prepare_models,
            commands::estimate_translation,
            commands::enqueue_translation,
            commands::list_jobs,
            commands::job_action,
            commands::clear_finished_jobs,
            commands::prioritize_chapter,
            commands::usage_summary,
            commands::editor_page,
            commands::update_region,
            commands::add_region,
            commands::delete_region,
            commands::move_region,
            commands::retry_region_ocr,
            commands::retranslate_region,
            commands::rerender_page,
            commands::reanalyze_page,
            commands::restore_original,
            commands::reset_page,
            commands::list_glossary,
            commands::upsert_glossary,
            commands::delete_glossary,
            commands::list_memory,
            commands::delete_memory,
            commands::source_status,
            commands::list_sources,
            commands::browse_source,
            commands::remote_manga,
            commands::enqueue_downloads,
            commands::plan_downloads,
            commands::duplicate_cleanup_plan,
            commands::clean_up_duplicates,
            commands::list_downloads,
            commands::download_action,
            commands::clear_finished_downloads,
            commands::set_source_password,
            commands::app_paths,
            commands::cache_size,
            commands::clear_thumbnails,
            commands::export_data,
            commands::dev_start_route,
            commands::dev_log,
            commands::source_overview,
            commands::setup_sources,
            commands::start_sources,
            commands::stop_sources,
            commands::search_all,
            commands::remote_chapter_count,
            commands::list_stores,
            commands::add_store,
            commands::validate_store_url,
            commands::apply_source_defaults,
            commands::remove_store,
            commands::list_extensions,
            commands::change_extension,
            commands::download_now,
            commands::download_size_hint,
            commands::provider_status,
            commands::installed_sources,
            commands::source_preferences,
            commands::set_source_preference,
        ]
    };
}

#[cfg(all(target_os = "macos", feature = "dev-snapshot"))]
fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    app_handlers!(devsnap::dev_snapshot)
}

#[cfg(not(all(target_os = "macos", feature = "dev-snapshot")))]
fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    app_handlers!()
}
