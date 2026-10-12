//! Online sources behind a small adapter interface. The implementation talks
//! to a user-run Suwayomi-Server (GraphQL), which hosts Mihon/Tachiyomi
//! extensions. Extensions are installed and configured in Suwayomi itself.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub supports_latest: bool,
    pub is_nsfw: bool,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteManga {
    pub source_id: String,
    pub manga_ref: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteChapter {
    pub chapter_ref: String,
    pub title: String,
    pub number: Option<f64>,
    pub scanlator: Option<String>,
    pub upload_date: Option<i64>,
    pub page_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MangaPage {
    pub mangas: Vec<RemoteManga>,
    pub has_next_page: bool,
}

#[async_trait]
pub trait SourceAdapter: Send + Sync {
    async fn ping(&self) -> AppResult<String>;
    async fn sources(&self) -> AppResult<Vec<SourceInfo>>;
    /// kind: search | popular | latest
    async fn browse(&self, source_id: &str, kind: &str, query: Option<&str>, page: u32) -> AppResult<MangaPage>;
    async fn manga(&self, manga_ref: &str) -> AppResult<RemoteManga>;
    async fn chapters(&self, manga_ref: &str) -> AppResult<Vec<RemoteChapter>>;
    /// Absolute URLs of the page images, in order.
    async fn page_urls(&self, chapter_ref: &str) -> AppResult<Vec<String>>;
    async fn fetch(&self, url: &str) -> AppResult<(Vec<u8>, Option<String>)>;
}

pub struct Suwayomi {
    base: String,
    auth: Option<(String, String)>,
    client: reqwest::Client,
}

impl Suwayomi {
    pub fn new(base: &str, auth: Option<(String, String)>) -> AppResult<Self> {
        let base = base.trim().trim_end_matches('/').to_string();
        let url = reqwest::Url::parse(&base).map_err(|e| AppError::invalid(format!("invalid Suwayomi address: {e}")))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AppError::invalid("Suwayomi address must be http(s)"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| AppError::new("network", e.to_string()))?;
        Ok(Self { base, auth, client })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn absolute(&self, u: &str) -> String {
        if u.starts_with("http://") || u.starts_with("https://") {
            u.to_string()
        } else {
            format!("{}/{}", self.base, u.trim_start_matches('/'))
        }
    }

    fn req(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Some((u, p)) => rb.basic_auth(u, Some(p)),
            None => rb,
        }
    }

    async fn gql(&self, query: &str, variables: Value) -> AppResult<Value> {
        let resp = self
            .req(self.client.post(format!("{}/api/graphql", self.base)))
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .map_err(|e| {
                if e.is_connect() {
                    AppError::new("source_offline", format!("Suwayomi is not reachable at {} — is it running?", self.base))
                } else {
                    AppError::new("network", e.to_string())
                }
            })?;
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AppError::new("source_auth", "Suwayomi rejected the credentials"));
        }
        let v: Value = resp.json().await.map_err(|e| AppError::new("source_error", format!("HTTP {status}: {e}")))?;
        if let Some(errs) = v["errors"].as_array().filter(|a| !a.is_empty()) {
            let msg = errs.iter().filter_map(|e| e["message"].as_str()).collect::<Vec<_>>().join("; ");
            return Err(AppError::new("source_error", msg));
        }
        Ok(v["data"].clone())
    }

    fn manga_from(&self, v: &Value) -> RemoteManga {
        RemoteManga {
            source_id: v["sourceId"].as_str().map(String::from).unwrap_or_else(|| v["sourceId"].to_string()),
            manga_ref: v["id"].to_string(),
            title: v["title"].as_str().unwrap_or_default().to_string(),
            thumbnail_url: v["thumbnailUrl"].as_str().map(|u| self.absolute(u)),
            author: v["author"].as_str().map(String::from),
            artist: v["artist"].as_str().map(String::from),
            description: v["description"].as_str().map(String::from),
            genres: v["genre"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect()).unwrap_or_default(),
            status: v["status"].as_str().map(String::from),
        }
    }
}

const MANGA_FIELDS: &str = "id sourceId title thumbnailUrl author artist description genre status";

#[async_trait]
impl SourceAdapter for Suwayomi {
    async fn ping(&self) -> AppResult<String> {
        let d = self.gql("query { aboutServer { name version } }", json!({})).await?;
        Ok(format!(
            "{} {}",
            d["aboutServer"]["name"].as_str().unwrap_or("Suwayomi"),
            d["aboutServer"]["version"].as_str().unwrap_or("")
        ))
    }

    async fn sources(&self) -> AppResult<Vec<SourceInfo>> {
        let d = self.gql("query { sources { nodes { id name lang supportsLatest isNsfw iconUrl displayName } } }", json!({})).await?;
        Ok(d["sources"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|s| SourceInfo {
                id: s["id"].as_str().map(String::from).unwrap_or_else(|| s["id"].to_string()),
                name: s["displayName"].as_str().or(s["name"].as_str()).unwrap_or_default().to_string(),
                lang: s["lang"].as_str().unwrap_or_default().to_string(),
                supports_latest: s["supportsLatest"].as_bool().unwrap_or(false),
                is_nsfw: s["isNsfw"].as_bool().unwrap_or(false),
                icon_url: s["iconUrl"].as_str().map(|u| self.absolute(u)),
            })
            .collect())
    }

    async fn browse(&self, source_id: &str, kind: &str, query: Option<&str>, page: u32) -> AppResult<MangaPage> {
        let t = match kind {
            "search" => "SEARCH",
            "latest" => "LATEST",
            _ => "POPULAR",
        };
        let q = format!(
            "mutation($source: LongString!, $type: FetchSourceMangaType!, $page: Int!, $query: String) {{
               fetchSourceManga(input: {{source: $source, type: $type, page: $page, query: $query}}) {{
                 hasNextPage mangas {{ {MANGA_FIELDS} }} }} }}"
        );
        let d = self.gql(&q, json!({"source": source_id, "type": t, "page": page.max(1), "query": query})).await?;
        let r = &d["fetchSourceManga"];
        Ok(MangaPage {
            mangas: r["mangas"].as_array().cloned().unwrap_or_default().iter().map(|m| self.manga_from(m)).collect(),
            has_next_page: r["hasNextPage"].as_bool().unwrap_or(false),
        })
    }

    async fn manga(&self, manga_ref: &str) -> AppResult<RemoteManga> {
        let id: i64 = manga_ref.parse().map_err(|_| AppError::invalid("bad manga id"))?;
        let q = format!("mutation($id: Int!) {{ fetchManga(input: {{id: $id}}) {{ manga {{ {MANGA_FIELDS} }} }} }}");
        let d = self.gql(&q, json!({"id": id})).await?;
        Ok(self.manga_from(&d["fetchManga"]["manga"]))
    }

    async fn chapters(&self, manga_ref: &str) -> AppResult<Vec<RemoteChapter>> {
        let id: i64 = manga_ref.parse().map_err(|_| AppError::invalid("bad manga id"))?;
        let q = "mutation($id: Int!) { fetchChapters(input: {mangaId: $id}) {
                   chapters { id name chapterNumber scanlator uploadDate pageCount sourceOrder } } }";
        let d = self.gql(q, json!({"id": id})).await?;
        let mut list: Vec<(i64, RemoteChapter)> = d["fetchChapters"]["chapters"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| {
                (
                    c["sourceOrder"].as_i64().unwrap_or(0),
                    RemoteChapter {
                        chapter_ref: c["id"].to_string(),
                        title: c["name"].as_str().unwrap_or_default().to_string(),
                        number: c["chapterNumber"].as_f64().filter(|n| *n >= 0.0),
                        scanlator: c["scanlator"].as_str().map(String::from),
                        upload_date: c["uploadDate"].as_str().and_then(|s| s.parse().ok()).or(c["uploadDate"].as_i64()),
                        page_count: c["pageCount"].as_i64().filter(|n| *n > 0),
                    },
                )
            })
            .collect();
        // sourceOrder 0 is the newest; present oldest first
        list.sort_by_key(|(o, _)| -o);
        Ok(list.into_iter().map(|(_, c)| c).collect())
    }

    async fn page_urls(&self, chapter_ref: &str) -> AppResult<Vec<String>> {
        let id: i64 = chapter_ref.parse().map_err(|_| AppError::invalid("bad chapter id"))?;
        let d = self.gql("mutation($id: Int!) { fetchChapterPages(input: {chapterId: $id}) { pages } }", json!({"id": id})).await?;
        let pages: Vec<String> = d["fetchChapterPages"]["pages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|p| p.as_str().map(|u| self.absolute(u)))
            .collect();
        if pages.is_empty() {
            return Err(AppError::new("source_error", "the source returned no pages for this chapter"));
        }
        Ok(pages)
    }

    async fn fetch(&self, url: &str) -> AppResult<(Vec<u8>, Option<String>)> {
        // Only fetch from the configured server.
        if !url.starts_with(&self.base) {
            return Err(AppError::invalid("refusing to fetch from another host"));
        }
        let resp = self
            .req(self.client.get(url))
            .send()
            .await
            .map_err(|e| AppError::new("network", e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::new("source_error", format!("HTTP {} for page image", resp.status())));
        }
        let ct = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(String::from);
        let bytes = resp.bytes().await.map_err(|e| AppError::new("network", e.to_string()))?;
        Ok((bytes.to_vec(), ct))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn graphql_roundtrip_against_mock_server() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(body_string_contains("fetchChapterPages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"fetchChapterPages": {"pages": [
                "/api/v1/manga/1/chapter/1/page/0", "/api/v1/manga/1/chapter/1/page/1"]}}})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(body_string_contains("fetchChapters"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"fetchChapters": {"chapters": [
                {"id": 11, "name": "Ch 2", "chapterNumber": 2.0, "sourceOrder": 0, "uploadDate": "1700000000000", "pageCount": -1},
                {"id": 10, "name": "Ch 1", "chapterNumber": 1.0, "sourceOrder": 1, "uploadDate": "1600000000000", "pageCount": 20}
            ]}}})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/graphql"))
            .and(body_string_contains("sources"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"errors": [{"message": "boom"}]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/manga/1/chapter/1/page/0"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "image/png").set_body_bytes(vec![1, 2, 3]))
            .mount(&server)
            .await;

        let s = Suwayomi::new(&server.uri(), None).unwrap();
        let pages = s.page_urls("5").await.unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages[0].starts_with(&server.uri()));
        let ch = s.chapters("1").await.unwrap();
        assert_eq!(ch[0].title, "Ch 1");
        assert_eq!(ch[1].page_count, None);
        assert_eq!(s.sources().await.unwrap_err().code, "source_error");
        let (bytes, ct) = s.fetch(&pages[0]).await.unwrap();
        assert_eq!((bytes, ct.as_deref()), (vec![1, 2, 3], Some("image/png")));
        assert!(s.fetch("http://evil.example/x").await.is_err());
    }

    #[tokio::test]
    async fn offline_server_is_reported() {
        let s = Suwayomi::new("http://127.0.0.1:9", None).unwrap();
        assert_eq!(s.ping().await.unwrap_err().code, "source_offline");
    }
}
