//! Catalog operations on top of Suwayomi: search across all installed
//! sources, conservative de-duplication, source health, extension stores and
//! extension (un)installation. Everything reported comes from the server.

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::sources::{RemoteManga, SourceAdapter, SourceInfo, Suwayomi};

/// Suwayomi's built-in source that reads files from its local folder.
pub const LOCAL_SOURCE_ID: &str = "0";

/// The official store's only extension, "Suwayomi": it reads another Suwayomi server's library,
/// it is not a manga website (verified on v2.4.2366 / extension 1.4.29).
pub const SUWAYOMI_BRIDGE_SOURCE_ID: &str = "3100117499901280806";

/// A source that searches a manga website (not the local folder or the server bridge).
pub fn is_website(source_id: &str) -> bool {
    source_id != LOCAL_SOURCE_ID && source_id != SUWAYOMI_BRIDGE_SOURCE_ID
}

/// Official Suwayomi store (verified 2026-10: contains one extension, "Suwayomi",
/// which connects to another Suwayomi server; it provides no manga websites).
pub const OFFICIAL_STORE_URL: &str = "https://raw.githubusercontent.com/suwayomi/tachiyomi-extension/repo/index.min.json";

// ---------------------------------------------------------------- search

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchEntry {
    pub source_id: String,
    pub source_name: String,
    pub lang: String,
    pub manga: RemoteManga,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchGroup {
    pub title: String,
    pub author: Option<String>,
    pub thumbnail_url: Option<String>,
    pub description: Option<String>,
    pub entries: Vec<SearchEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceFailure {
    pub source_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    pub groups: Vec<SearchGroup>,
    pub sources_searched: usize,
    pub online_sources: usize,
    /// Installed sources that are manga websites.
    pub website_sources: usize,
    pub failures: Vec<SourceFailure>,
    /// A newer search replaced this one before it finished.
    pub cancelled: bool,
}

/// Letters and digits only, lower-cased: "Kaguya-sama: Love is War" == "kaguya sama love is war".
pub fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Merge results only when both the normalised title and a known author match.
/// Similar or identical titles without author information stay separate.
pub fn group_results(entries: Vec<SearchEntry>) -> Vec<SearchGroup> {
    let mut groups: Vec<(Option<String>, SearchGroup)> = vec![];
    for e in entries {
        let author = e.manga.author.as_deref().map(normalize).filter(|a| !a.is_empty());
        let key = author.as_ref().map(|a| format!("{}|{}", normalize(&e.manga.title), a));
        if let Some(k) = &key {
            if let Some((_, g)) = groups.iter_mut().find(|(gk, _)| gk.as_ref() == Some(k)) {
                if !g.entries.iter().any(|x| x.source_id == e.source_id) {
                    if g.thumbnail_url.is_none() {
                        g.thumbnail_url = e.manga.thumbnail_url.clone();
                    }
                    if g.description.as_deref().map(str::is_empty).unwrap_or(true) {
                        g.description = e.manga.description.clone();
                    }
                    g.entries.push(e);
                    continue;
                }
            }
        }
        groups.push((
            key,
            SearchGroup {
                title: e.manga.title.clone(),
                author: e.manga.author.clone(),
                thumbnail_url: e.manga.thumbnail_url.clone(),
                description: e.manga.description.clone(),
                entries: vec![e],
            },
        ));
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// Search every installed source concurrently. `generation` lets a newer search
/// cancel sources that have not started yet.
pub async fn search_all(
    adapter: Arc<dyn SourceAdapter>,
    query: &str,
    generation: u64,
    latest: Arc<AtomicU64>,
) -> AppResult<SearchOutcome> {
    let query = query.trim();
    if query.is_empty() {
        return Err(AppError::invalid("empty search"));
    }
    let sources = adapter.sources().await?;
    let online_sources = sources.iter().filter(|s| s.id != LOCAL_SOURCE_ID).count();
    let website_sources = sources.iter().filter(|s| is_website(&s.id)).count();
    if sources.is_empty() {
        return Err(AppError::new("no_sources", "no manga sources are installed"));
    }
    let total = sources.len();
    let results: Vec<(SourceInfo, Option<AppResult<Vec<RemoteManga>>>)> = stream::iter(sources)
        .map(|s| {
            let adapter = adapter.clone();
            let latest = latest.clone();
            async move {
                if latest.load(Ordering::SeqCst) != generation {
                    return (s, None);
                }
                let r = tokio::time::timeout(Duration::from_secs(25), adapter.browse(&s.id, "search", Some(query), 1)).await;
                let r = match r {
                    Ok(Ok(page)) => Ok(page.mangas),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(AppError::new("timeout", "the source did not answer in time")),
                };
                (s, Some(r))
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let cancelled = latest.load(Ordering::SeqCst) != generation;
    let mut entries = vec![];
    let mut failures = vec![];
    let mut searched = 0;
    // stable order: sources in the order the server listed them
    let mut results = results;
    results.sort_by_key(|(s, _)| s.name.to_lowercase());
    for (src, r) in results {
        match r {
            None => {}
            Some(Ok(list)) => {
                searched += 1;
                entries.extend(list.into_iter().map(|m| SearchEntry {
                    source_id: src.id.clone(),
                    source_name: src.name.clone(),
                    lang: src.lang.clone(),
                    manga: m,
                }));
            }
            Some(Err(e)) => {
                searched += 1;
                failures.push(SourceFailure { source_name: src.name.clone(), message: e.message });
            }
        }
    }
    let _ = total;
    Ok(SearchOutcome { groups: group_results(entries), sources_searched: searched, online_sources, website_sources, failures, cancelled })
}

// ---------------------------------------------------------------- health

#[derive(Debug, Clone, Serialize)]
pub struct SourceHealth {
    /// ok | offline | auth | disabled | error
    pub server: String,
    pub server_version: Option<String>,
    pub message: Option<String>,
    pub sources: usize,
    pub online_sources: usize,
    pub website_sources: usize,
    pub stores: usize,
    pub installed_extensions: usize,
}

pub async fn health(s: &Suwayomi) -> SourceHealth {
    let mut h = SourceHealth {
        server: "error".into(),
        server_version: None,
        message: None,
        sources: 0,
        online_sources: 0,
        website_sources: 0,
        stores: 0,
        installed_extensions: 0,
    };
    match s.ping().await {
        Ok(v) => {
            h.server = "ok".into();
            h.server_version = Some(v);
        }
        Err(e) => {
            h.server = match e.code.as_str() {
                "source_offline" => "offline",
                "source_auth" => "auth",
                _ => "error",
            }
            .into();
            h.message = Some(e.message);
            return h;
        }
    }
    if let Ok(list) = s.sources().await {
        h.sources = list.len();
        h.online_sources = list.iter().filter(|x| x.id != LOCAL_SOURCE_ID).count();
        h.website_sources = list.iter().filter(|x| is_website(&x.id)).count();
    }
    if let Ok(stores) = list_stores(s).await {
        h.stores = stores.len();
    }
    if let Ok(exts) = list_extensions(s).await {
        h.installed_extensions = exts.iter().filter(|e| e.is_installed).count();
    }
    h
}

// ---------------------------------------------------------------- stores & extensions

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtensionStore {
    pub name: String,
    pub index_url: String,
    pub is_legacy: bool,
    pub badge_label: String,
    pub contact_website: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Extension {
    pub pkg_name: String,
    pub name: String,
    pub lang: String,
    pub version_name: String,
    pub is_installed: bool,
    pub has_update: bool,
    pub is_obsolete: bool,
    pub is_nsfw: bool,
    pub store_index_url: Option<String>,
    pub icon_url: Option<String>,
}

const STORE_FIELDS: &str = "name indexUrl isLegacy badgeLabel contactWebsite";
const EXT_FIELDS: &str = "pkgName name lang versionName isInstalled hasUpdate isObsolete isNsfw storeIndexUrl iconUrl";

fn store_from(v: &serde_json::Value) -> ExtensionStore {
    ExtensionStore {
        name: v["name"].as_str().unwrap_or_default().into(),
        index_url: v["indexUrl"].as_str().unwrap_or_default().into(),
        is_legacy: v["isLegacy"].as_bool().unwrap_or(false),
        badge_label: v["badgeLabel"].as_str().unwrap_or_default().into(),
        contact_website: v["contactWebsite"].as_str().unwrap_or_default().into(),
    }
}

fn ext_from(s: &Suwayomi, v: &serde_json::Value) -> Extension {
    Extension {
        pkg_name: v["pkgName"].as_str().unwrap_or_default().into(),
        name: v["name"].as_str().unwrap_or_default().into(),
        lang: v["lang"].as_str().unwrap_or_default().into(),
        version_name: v["versionName"].as_str().unwrap_or_default().into(),
        is_installed: v["isInstalled"].as_bool().unwrap_or(false),
        has_update: v["hasUpdate"].as_bool().unwrap_or(false),
        is_obsolete: v["isObsolete"].as_bool().unwrap_or(false),
        is_nsfw: v["isNsfw"].as_bool().unwrap_or(false),
        store_index_url: v["storeIndexUrl"].as_str().map(String::from),
        icon_url: v["iconUrl"].as_str().map(|u| s.absolute_url(u)),
    }
}

pub async fn list_stores(s: &Suwayomi) -> AppResult<Vec<ExtensionStore>> {
    let d = s.graphql(&format!("query {{ extensionStores {{ nodes {{ {STORE_FIELDS} }} }} }}"), json!({})).await?;
    Ok(d["extensionStores"]["nodes"].as_array().cloned().unwrap_or_default().iter().map(store_from).collect())
}

/// Client-side checks before asking the server (which fetches and validates the index itself).
pub fn validate_store_url(url: &str) -> AppResult<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(AppError::invalid("enter the address of an extension store index (a .json file)"));
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| AppError::invalid("this is not a valid web address"))?;
    if parsed.scheme() != "https" {
        return Err(AppError::invalid("extension stores must use https"));
    }
    if parsed.host_str().is_none() {
        return Err(AppError::invalid("the address has no host"));
    }
    if !parsed.path().ends_with(".json") {
        return Err(AppError::invalid("the address must point to the store's index file (ending in .json), not to a website"));
    }
    Ok(url.to_string())
}

pub async fn add_store(s: &Suwayomi, url: &str) -> AppResult<ExtensionStore> {
    let url = validate_store_url(url)?;
    let d = s
        .graphql(
            &format!("mutation($u: String!) {{ addExtensionStore(input: {{indexUrl: $u}}) {{ extensionStore {{ {STORE_FIELDS} }} }} }}"),
            json!({"u": url}),
        )
        .await
        .map_err(|e| AppError::new("store_rejected", format!("the server could not use this store: {}", e.message)))?;
    let v = &d["addExtensionStore"]["extensionStore"];
    if v.is_null() {
        return Err(AppError::new("store_rejected", "the server did not confirm the store"));
    }
    Ok(store_from(v))
}

pub async fn remove_store(s: &Suwayomi, index_url: &str) -> AppResult<()> {
    s.graphql(
        "mutation($u: String!) { removeExtensionStore(input: {indexUrl: $u}) { extensionStore { indexUrl } } }",
        json!({"u": index_url}),
    )
    .await?;
    Ok(())
}

/// Re-download every store's index and return the resulting extension list.
pub async fn refresh_extensions(s: &Suwayomi) -> AppResult<Vec<Extension>> {
    let d = s.graphql(&format!("mutation {{ fetchExtensions(input: {{}}) {{ extensions {{ {EXT_FIELDS} }} }} }}"), json!({})).await?;
    Ok(d["fetchExtensions"]["extensions"].as_array().cloned().unwrap_or_default().iter().map(|v| ext_from(s, v)).collect())
}

pub async fn list_extensions(s: &Suwayomi) -> AppResult<Vec<Extension>> {
    let d = s.graphql(&format!("query {{ extensions {{ nodes {{ {EXT_FIELDS} }} }} }}"), json!({})).await?;
    Ok(d["extensions"]["nodes"].as_array().cloned().unwrap_or_default().iter().map(|v| ext_from(s, v)).collect())
}

/// action: install | uninstall | update. Succeeds only when the server reports the new state.
pub async fn change_extension(s: &Suwayomi, pkg_name: &str, action: &str) -> AppResult<Extension> {
    let patch = match action {
        "install" => json!({"install": true}),
        "uninstall" => json!({"uninstall": true}),
        "update" => json!({"update": true}),
        _ => return Err(AppError::invalid("unknown extension action")),
    };
    let d = s
        .graphql(
            &format!("mutation($id: String!, $p: UpdateExtensionPatchInput!) {{ updateExtension(input: {{id: $id, patch: $p}}) {{ extension {{ {EXT_FIELDS} }} }} }}"),
            json!({"id": pkg_name, "p": patch}),
        )
        .await
        .map_err(|e| AppError::new("extension_failed", e.message))?;
    let v = &d["updateExtension"]["extension"];
    if v.is_null() {
        return Err(AppError::new("extension_failed", "the server did not return the extension"));
    }
    let ext = ext_from(s, v);
    let ok = match action {
        "install" => ext.is_installed,
        "uninstall" => !ext.is_installed,
        _ => ext.is_installed && !ext.has_update,
    };
    if !ok {
        return Err(AppError::new("extension_failed", format!("the server did not confirm the {action}")));
    }
    Ok(ext)
}

// ---------------------------------------------------------------- source settings

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourcePreference {
    /// Index in the source's preference list (what the server's update mutation expects).
    pub position: usize,
    /// text | switch | checkbox | list | multi
    pub kind: String,
    pub key: String,
    pub title: String,
    pub summary: String,
    pub value: serde_json::Value,
    pub entries: Vec<String>,
    pub entry_values: Vec<String>,
    /// Looks like a secret (password/token): the UI masks it.
    pub secret: bool,
}

const PREF_FIELDS: &str = "preferences { __typename
    ... on EditTextPreference { key title summary text: currentValue }
    ... on SwitchPreference { key title summary on: currentValue }
    ... on CheckBoxPreference { key title summary checked: currentValue }
    ... on ListPreference { key title summary entries entryValues selected: currentValue }
    ... on MultiSelectListPreference { key title summary entries entryValues values: currentValue } }";

fn prefs_from(v: &serde_json::Value) -> Vec<SourcePreference> {
    let strs = |x: &serde_json::Value| -> Vec<String> {
        x.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
    };
    v.as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (kind, value) = match p["__typename"].as_str().unwrap_or("") {
                "EditTextPreference" => ("text", p["text"].clone()),
                "SwitchPreference" => ("switch", p["on"].clone()),
                "CheckBoxPreference" => ("checkbox", p["checked"].clone()),
                "ListPreference" => ("list", p["selected"].clone()),
                "MultiSelectListPreference" => ("multi", p["values"].clone()),
                _ => ("unknown", serde_json::Value::Null),
            };
            let key = p["key"].as_str().unwrap_or_default().to_string();
            let title = p["title"].as_str().unwrap_or_default().to_string();
            let lower = format!("{key} {title}").to_lowercase();
            SourcePreference {
                position: i,
                kind: kind.into(),
                secret: kind == "text" && (lower.contains("password") || lower.contains("token") || lower.contains("secret")),
                key,
                title,
                summary: p["summary"].as_str().unwrap_or_default().to_string(),
                value,
                entries: strs(&p["entries"]),
                entry_values: strs(&p["entryValues"]),
            }
        })
        .collect()
}

pub async fn source_preferences(s: &Suwayomi, source_id: &str) -> AppResult<Vec<SourcePreference>> {
    let d = s.graphql(&format!("query($id: LongString!) {{ source(id: $id) {{ {PREF_FIELDS} }} }}"), json!({"id": source_id})).await?;
    Ok(prefs_from(&d["source"]["preferences"]))
}

/// Update one preference; returns the server's new preference list.
pub async fn set_source_preference(
    s: &Suwayomi,
    source_id: &str,
    position: usize,
    kind: &str,
    value: serde_json::Value,
) -> AppResult<Vec<SourcePreference>> {
    let field = match kind {
        "text" => "editTextState",
        "switch" => "switchState",
        "checkbox" => "checkBoxState",
        "list" => "listState",
        "multi" => "multiSelectState",
        _ => return Err(AppError::invalid("unsupported preference type")),
    };
    let mut change = json!({"position": position});
    change[field] = value;
    let d = s
        .graphql(
            &format!("mutation($id: LongString!, $c: SourcePreferenceChangeInput!) {{ updateSourcePreference(input: {{source: $id, change: $c}}) {{ {PREF_FIELDS} }} }}"),
            json!({"id": source_id, "c": change}),
        )
        .await?;
    Ok(prefs_from(&d["updateSourcePreference"]["preferences"]))
}

/// Change an extension, then re-read the server's source list so new sources appear immediately.
pub async fn change_extension_and_sources(s: &Suwayomi, pkg_name: &str, action: &str) -> AppResult<(Extension, Vec<SourceInfo>)> {
    let ext = change_extension(s, pkg_name, action).await?;
    let sources = s.sources().await?;
    Ok((ext, sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn m(title: &str, author: Option<&str>, src: &str) -> SearchEntry {
        SearchEntry {
            source_id: src.into(),
            source_name: format!("S{src}"),
            lang: "en".into(),
            manga: RemoteManga {
                source_id: src.into(),
                manga_ref: format!("{src}-{title}"),
                title: title.into(),
                thumbnail_url: None,
                author: author.map(String::from),
                artist: None,
                description: None,
                genres: vec![],
                status: None,
            },
        }
    }

    #[test]
    fn merges_only_on_title_and_author() {
        let g = group_results(vec![
            m("Kaguya-sama: Love is War", Some("Aka Akasaka"), "1"),
            m("kaguya sama love is war", Some("AKA AKASAKA"), "2"),
            m("Kaguya-sama: Love is War", None, "3"),         // no author: separate
            m("Kaguya-sama: Love is War?", Some("Someone Else"), "4"), // other author: separate
            m("Kaguya-sama Love", Some("Aka Akasaka"), "5"),   // different title: separate
        ]);
        assert_eq!(g.len(), 4);
        assert_eq!(g[0].entries.len(), 2);
        assert_eq!(g[0].entries[1].source_id, "2");
    }

    #[test]
    fn same_source_twice_is_not_merged() {
        let g = group_results(vec![m("A", Some("x"), "1"), m("A", Some("x"), "1")]);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn bridge_and_local_are_not_websites() {
        assert!(!is_website(LOCAL_SOURCE_ID));
        assert!(!is_website(SUWAYOMI_BRIDGE_SOURCE_ID));
        assert!(is_website("1234"));
    }

    #[test]
    fn store_url_validation() {
        assert!(validate_store_url(OFFICIAL_STORE_URL).is_ok());
        assert_eq!(validate_store_url("").unwrap_err().code, "invalid");
        assert!(validate_store_url("not a url").unwrap_err().message.contains("valid"));
        assert!(validate_store_url("http://example.com/index.min.json").unwrap_err().message.contains("https"));
        assert!(validate_store_url("https://example.com/").unwrap_err().message.contains(".json"));
    }

    async fn server_with(handlers: Vec<(&'static str, serde_json::Value)>) -> MockServer {
        let server = MockServer::start().await;
        for (needle, body) in handlers {
            Mock::given(method("POST"))
                .and(path("/api/graphql"))
                .and(body_string_contains(needle))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        server
    }

    fn manga_json(id: i64, title: &str, author: &str) -> serde_json::Value {
        json!({"id": id, "sourceId": "x", "title": title, "thumbnailUrl": null, "author": author, "artist": null,
               "description": null, "genre": [], "status": "ONGOING"})
    }

    #[tokio::test]
    async fn search_across_sources_with_failure_and_dedupe() {
        let server = server_with(vec![
            ("sources", json!({"data": {"sources": {"nodes": [
                {"id": "11", "name": "Alpha", "displayName": "Alpha", "lang": "en", "supportsLatest": true, "isNsfw": false, "iconUrl": ""},
                {"id": "22", "name": "Beta", "displayName": "Beta", "lang": "en", "supportsLatest": true, "isNsfw": false, "iconUrl": ""},
                {"id": "33", "name": "Gamma", "displayName": "Gamma", "lang": "en", "supportsLatest": true, "isNsfw": false, "iconUrl": ""}
            ]}}})),
            ("\"11\"", json!({"data": {"fetchSourceManga": {"hasNextPage": false, "mangas": [manga_json(1, "Same Title", "Author")]}}})),
            ("\"22\"", json!({"data": {"fetchSourceManga": {"hasNextPage": false, "mangas": [manga_json(2, "Same title!", "author"), manga_json(3, "Other", "Z")]}}})),
            ("\"33\"", json!({"errors": [{"message": "Exception while fetching data (/fetchSourceManga) : Cloudflare\r\nstack"}]})),
        ])
        .await;
        let a: Arc<dyn SourceAdapter> = Arc::new(Suwayomi::new(&server.uri(), None).unwrap());
        let latest = Arc::new(AtomicU64::new(1));
        let out = search_all(a, "same", 1, latest).await.unwrap();
        assert_eq!(out.sources_searched, 3);
        assert_eq!(out.website_sources, 3);
        assert_eq!(out.failures.len(), 1);
        assert_eq!(out.failures[0].message, "Cloudflare");
        assert_eq!(out.groups.len(), 2);
        assert_eq!(out.groups[0].entries.len(), 2, "same title + same author from two sources merged");
        assert!(!out.cancelled);
    }

    #[tokio::test]
    async fn search_with_no_sources_and_no_results() {
        let empty = server_with(vec![("sources", json!({"data": {"sources": {"nodes": []}}}))]).await;
        let a: Arc<dyn SourceAdapter> = Arc::new(Suwayomi::new(&empty.uri(), None).unwrap());
        let e = search_all(a, "x", 1, Arc::new(AtomicU64::new(1))).await.unwrap_err();
        assert_eq!(e.code, "no_sources");

        let none = server_with(vec![
            ("sources", json!({"data": {"sources": {"nodes": [{"id": "0", "name": "Local source", "lang": "localsourcelang", "supportsLatest": false, "isNsfw": false}]}}})),
            ("fetchSourceManga", json!({"data": {"fetchSourceManga": {"hasNextPage": false, "mangas": []}}})),
        ])
        .await;
        let a: Arc<dyn SourceAdapter> = Arc::new(Suwayomi::new(&none.uri(), None).unwrap());
        let out = search_all(a, "x", 1, Arc::new(AtomicU64::new(1))).await.unwrap();
        assert!(out.groups.is_empty());
        assert_eq!(out.online_sources, 0);
        assert_eq!(out.website_sources, 0);
        assert_eq!(out.sources_searched, 1);
    }

    #[tokio::test]
    async fn superseded_search_is_cancelled() {
        let server = server_with(vec![
            ("sources", json!({"data": {"sources": {"nodes": [{"id": "11", "name": "A", "lang": "en"}]}}})),
            ("fetchSourceManga", json!({"data": {"fetchSourceManga": {"hasNextPage": false, "mangas": [manga_json(1, "T", "A")]}}})),
        ])
        .await;
        let a: Arc<dyn SourceAdapter> = Arc::new(Suwayomi::new(&server.uri(), None).unwrap());
        let out = search_all(a, "t", 1, Arc::new(AtomicU64::new(2))).await.unwrap();
        assert!(out.cancelled);
        assert!(out.groups.is_empty());
    }

    #[tokio::test]
    async fn stores_and_extension_lifecycle() {
        let server = server_with(vec![
            ("extensionStores", json!({"data": {"extensionStores": {"nodes": []}}})),
            ("addExtensionStore", json!({"data": {"addExtensionStore": {"extensionStore": {"name": "Repo", "indexUrl": "https://e.x/repo.json", "isLegacy": true, "badgeLabel": "", "contactWebsite": ""}}}})),
            ("fetchExtensions", json!({"data": {"fetchExtensions": {"extensions": [
                {"pkgName": "p.a", "name": "A", "lang": "en", "versionName": "1", "isInstalled": false, "hasUpdate": false, "isObsolete": false, "isNsfw": false, "storeIndexUrl": "https://e.x/repo.json", "iconUrl": "/api/v1/extension/icon/p.a"}
            ]}}})),
            ("\"p.ok\"", json!({"data": {"updateExtension": {"extension":
                {"pkgName": "p.ok", "name": "OK", "lang": "en", "versionName": "1", "isInstalled": true, "hasUpdate": false, "isObsolete": false, "isNsfw": false}}}})),
            ("\"p.bad\"", json!({"data": {"updateExtension": {"extension":
                {"pkgName": "p.bad", "name": "Bad", "lang": "en", "versionName": "1", "isInstalled": false, "hasUpdate": false, "isObsolete": false, "isNsfw": false}}}})),
            ("\"p.err\"", json!({"errors": [{"message": "Exception while fetching data (/updateExtension) : apk download failed\r\nat"}]})),
        ])
        .await;
        let s = Suwayomi::new(&server.uri(), None).unwrap();
        assert!(list_stores(&s).await.unwrap().is_empty(), "empty store list");
        let st = add_store(&s, "https://e.x/index.min.json").await.unwrap();
        assert_eq!(st.index_url, "https://e.x/repo.json");
        let exts = refresh_extensions(&s).await.unwrap();
        assert_eq!(exts.len(), 1);
        assert!(exts[0].icon_url.as_deref().unwrap().starts_with(&server.uri()));
        assert!(change_extension(&s, "p.ok", "install").await.unwrap().is_installed);
        assert_eq!(change_extension(&s, "p.bad", "install").await.unwrap_err().code, "extension_failed", "unconfirmed install is a failure");
        let e = change_extension(&s, "p.err", "install").await.unwrap_err();
        assert_eq!(e.message, "apk download failed");
    }

    #[tokio::test]
    async fn sources_refresh_after_extension_install() {
        let server = MockServer::start().await;
        // before the install the server lists only the local source; afterwards a new one
        Mock::given(method("POST")).and(path("/api/graphql")).and(body_string_contains("updateExtension"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"updateExtension": {"extension":
                {"pkgName": "p.new", "name": "New", "lang": "en", "versionName": "1", "isInstalled": true, "hasUpdate": false, "isObsolete": false, "isNsfw": false}}}})))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/api/graphql")).and(body_string_contains("sources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"sources": {"nodes": [
                {"id": "0", "name": "Local source", "lang": "localsourcelang"},
                {"id": "77", "name": "New", "lang": "en"}]}}})))
            .mount(&server).await;
        let s = Suwayomi::new(&server.uri(), None).unwrap();
        let (ext, sources) = change_extension_and_sources(&s, "p.new", "install").await.unwrap();
        assert!(ext.is_installed);
        assert_eq!(sources.iter().filter(|x| x.id != LOCAL_SOURCE_ID).count(), 1);
    }

    #[tokio::test]
    async fn source_preferences_roundtrip() {
        let prefs = json!([
            {"__typename": "EditTextPreference", "key": "Server URL Address", "title": "Server URL Address", "summary": "", "text": ""},
            {"__typename": "ListPreference", "key": "Login Mode", "title": "Login Mode", "summary": "None", "entries": ["None", "Basic"], "entryValues": ["NONE", "BASIC_AUTH"], "selected": "NONE"},
            {"__typename": "EditTextPreference", "key": "Password", "title": "Password", "summary": "", "text": ""},
            {"__typename": "CheckBoxPreference", "key": "Fetch", "title": "Fetch", "summary": "", "checked": true}
        ]);
        let mut after = prefs.clone();
        after[0]["text"] = json!("http://127.0.0.1:4568");
        let server = server_with(vec![
            ("updateSourcePreference", json!({"data": {"updateSourcePreference": {"preferences": after}}})),
            ("source(id", json!({"data": {"source": {"preferences": prefs}}})),
        ])
        .await;
        let s = Suwayomi::new(&server.uri(), None).unwrap();
        let p = source_preferences(&s, "31").await.unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!((p[1].kind.as_str(), p[1].entry_values.len()), ("list", 2));
        assert!(p[2].secret && !p[0].secret);
        let p = set_source_preference(&s, "31", 0, "text", json!("http://127.0.0.1:4568")).await.unwrap();
        assert_eq!(p[0].value, "http://127.0.0.1:4568");
        assert!(set_source_preference(&s, "31", 0, "weird", json!(1)).await.is_err());
    }

    #[tokio::test]
    async fn store_rejected_by_server() {
        let server = server_with(vec![(
            "addExtensionStore",
            json!({"data": {"addExtensionStore": null}, "errors": [{"message": "Exception while fetching data (/addExtensionStore) : HTTP error 404\r\n\r\nstack"}]}),
        )])
        .await;
        let s = Suwayomi::new(&server.uri(), None).unwrap();
        let e = add_store(&s, "https://example.com/nope.json").await.unwrap_err();
        assert_eq!(e.code, "store_rejected");
        assert!(e.message.contains("HTTP error 404"));
    }

    #[tokio::test]
    async fn health_reports_offline_server() {
        let s = Suwayomi::new("http://127.0.0.1:9", None).unwrap();
        let h = health(&s).await;
        assert_eq!(h.server, "offline");
        assert_eq!(h.sources, 0);
    }
}
