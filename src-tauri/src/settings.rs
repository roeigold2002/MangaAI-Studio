//! User settings, stored as one JSON document in the `setting` table.
//! Secrets are NOT stored here (see `secrets.rs`).

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProviderConfig {
    /// Stable id, also the Keychain account name for its API key.
    pub id: String,
    /// anthropic | openai | gemini | openai_compatible | ollama | claude_cli
    pub kind: String,
    pub label: String,
    pub model: String,
    /// Base URL for openai_compatible / ollama / overrides.
    pub endpoint: String,
    pub max_concurrency: u32,
    pub timeout_secs: u64,
    pub max_retries: u32,
    /// Send the page image along with OCR text (vision models only).
    pub send_images: bool,
    /// USD per million tokens; 0 = unknown (no cost estimate shown).
    pub price_input_per_mtok: f64,
    pub price_output_per_mtok: f64,
    pub max_output_tokens: u32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: String::new(),
            label: String::new(),
            model: String::new(),
            endpoint: String::new(),
            max_concurrency: 2,
            timeout_secs: 180,
            max_retries: 3,
            send_images: true,
            price_input_per_mtok: 0.0,
            price_output_per_mtok: 0.0,
            max_output_tokens: 8000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TranslationSettings {
    pub active_provider: String,
    pub providers: Vec<ProviderConfig>,
    pub target_lang: String,
    /// fast | balanced | best
    pub quality: String,
    pub pages_per_request: u32,
    pub context_lines: u32,
    pub confirm_cost_above_usd: f64,
    /// Hard stop for a single job; 0 = no limit.
    pub job_cost_limit_usd: f64,
    /// keep | translate
    pub sfx_mode: String,
    /// Only allow providers that run on this computer.
    pub local_only: bool,
    pub auto_translate_next: bool,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            active_provider: "claude_cli".into(),
            providers: default_providers(),
            target_lang: "he".into(),
            quality: "balanced".into(),
            pages_per_request: 2,
            context_lines: 40,
            confirm_cost_above_usd: 1.0,
            job_cost_limit_usd: 0.0,
            sfx_mode: "translate".into(),
            local_only: false,
            auto_translate_next: false,
        }
    }
}

/// Prices are defaults the user can edit; they are only used for estimates.
pub fn default_providers() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            id: "claude_cli".into(),
            kind: "claude_cli".into(),
            label: "Claude Code (המנוי שלך)".into(),
            model: "opus".into(),
            // two page batches translated at once (each is its own CLI process)
            max_concurrency: 2,
            timeout_secs: 300,
            ..Default::default()
        },
        ProviderConfig {
            id: "anthropic".into(),
            kind: "anthropic".into(),
            label: "Anthropic API".into(),
            model: "claude-opus-5-5".into(),
            endpoint: "https://api.anthropic.com".into(),
            price_input_per_mtok: 4.0,
            price_output_per_mtok: 20.0,
            ..Default::default()
        },
        ProviderConfig {
            id: "openai".into(),
            kind: "openai".into(),
            label: "OpenAI API".into(),
            model: String::new(),
            endpoint: "https://api.openai.com/v1".into(),
            ..Default::default()
        },
        ProviderConfig {
            id: "gemini".into(),
            kind: "gemini".into(),
            label: "Google Gemini API".into(),
            model: String::new(),
            endpoint: "https://generativelanguage.googleapis.com/v1beta".into(),
            ..Default::default()
        },
        ProviderConfig {
            id: "ollama".into(),
            kind: "ollama".into(),
            label: "Ollama (מקומי)".into(),
            model: String::new(),
            endpoint: "http://127.0.0.1:11434".into(),
            max_concurrency: 1,
            timeout_secs: 600,
            ..Default::default()
        },
        ProviderConfig {
            id: "openai_compatible".into(),
            kind: "openai_compatible".into(),
            label: "OpenAI-compatible (LM Studio / vLLM / אחר)".into(),
            model: String::new(),
            endpoint: "http://127.0.0.1:1234/v1".into(),
            max_concurrency: 1,
            send_images: false,
            ..Default::default()
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RenderSettings {
    pub font: String,
    pub font_loud: String,
    pub min_font_px: u32,
    pub max_font_px: u32,
    /// Fraction of the box kept free on each side.
    pub padding: f32,
    pub line_spacing: f32,
    pub stroke_px: u32,
    /// lama | opencv | fill
    pub inpaint: String,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            font: "VarelaRound-Regular.ttf".into(),
            font_loud: "SecularOne-Regular.ttf".into(),
            min_font_px: 12,
            max_font_px: 64,
            padding: 0.10,
            line_spacing: 1.08,
            stroke_px: 3,
            inpaint: "lama".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OcrSettings {
    pub detector_threshold: f32,
    /// auto | cpu | mps | cuda
    pub device: String,
}

impl Default for OcrSettings {
    fn default() -> Self {
        Self { detector_threshold: 0.35, device: "auto".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ReaderSettings {
    /// single | double | vertical
    pub mode: String,
    /// rtl | ltr (page navigation direction)
    pub direction: String,
    /// height | width | original
    pub fit: String,
    pub show_translated: bool,
    pub preload: u32,
    /// In double-page mode, show the first page alone (cover).
    pub double_cover_alone: bool,
    pub vertical_gap: u32,
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            mode: "single".into(),
            direction: "rtl".into(),
            fit: "height".into(),
            show_translated: true,
            preload: 3,
            double_cover_alone: true,
            vertical_gap: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SourceSettings {
    pub suwayomi_enabled: bool,
    pub suwayomi_url: String,
    pub suwayomi_user: String,
    pub download_concurrency: u32,
    /// The app installed Suwayomi itself and starts/stops it.
    pub managed: bool,
    pub managed_port: u16,
    /// Source languages to search, in order of preference (multi-language extensions
    /// such as MangaDex install one source per language).
    pub languages: Vec<String>,
    /// Extensions (package names) the one-click setup installs from the default stores,
    /// so a new install can search right away. Edit or empty the list to change it.
    pub default_extensions: Vec<String>,
}

/// MangaDex and MangaFire from the Keiyoushi store (multi-language; `languages` picks
/// which of their per-language sources are searched).
pub const DEFAULT_EXTENSIONS: &[&str] = &["eu.kanade.tachiyomi.extension.all.mangadex", "eu.kanade.tachiyomi.extension.all.mangafire"];

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            suwayomi_enabled: true,
            suwayomi_url: "http://127.0.0.1:4567".into(),
            suwayomi_user: String::new(),
            download_concurrency: 2,
            managed: false,
            managed_port: 4567,
            languages: vec!["en".into()],
            default_extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    /// dark | light
    pub theme: String,
    pub library_view: String,
    pub reader: ReaderSettings,
    pub translation: TranslationSettings,
    pub render: RenderSettings,
    pub ocr: OcrSettings,
    pub sources: SourceSettings,
    /// User acknowledged that cloud translation sends data to the provider.
    pub cloud_notice_accepted: bool,
    /// The first-run welcome was shown (or dismissed).
    pub onboarding_done: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            library_view: "grid".into(),
            reader: ReaderSettings::default(),
            translation: TranslationSettings::default(),
            render: RenderSettings::default(),
            ocr: OcrSettings::default(),
            sources: SourceSettings::default(),
            cloud_notice_accepted: false,
            onboarding_done: false,
        }
    }
}

impl AppSettings {
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.translation.providers.iter().find(|p| p.id == id)
    }
    pub fn active_provider(&self) -> Option<&ProviderConfig> {
        self.provider(&self.translation.active_provider)
    }
}

pub fn load(db: &Db) -> AppResult<AppSettings> {
    let raw: Option<String> = db.with(|c| {
        use rusqlite::OptionalExtension;
        Ok(c.query_row("SELECT value FROM setting WHERE key = 'app'", [], |r| r.get(0)).optional()?)
    })?;
    let mut s: AppSettings = match raw {
        Some(r) => serde_json::from_str(&r).unwrap_or_else(|e| {
            tracing::warn!("settings unreadable, using defaults: {e}");
            AppSettings::default()
        }),
        None => AppSettings::default(),
    };
    // New built-in providers appear for users with older settings.
    for d in default_providers() {
        if !s.translation.providers.iter().any(|p| p.id == d.id) {
            s.translation.providers.push(d);
        }
    }
    Ok(s)
}

pub fn save(db: &Db, s: &AppSettings) -> AppResult<()> {
    let json = serde_json::to_string(s)?;
    db.with(|c| {
        c.execute(
            "INSERT INTO setting (key, value) VALUES ('app', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [json],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_forward_compatible() {
        let db = Db::open_in_memory().unwrap();
        let mut s = load(&db).unwrap();
        assert_eq!(s.translation.target_lang, "he");
        s.reader.direction = "ltr".into();
        save(&db, &s).unwrap();
        assert_eq!(load(&db).unwrap().reader.direction, "ltr");
        // unknown/missing fields fall back to defaults
        db.with(|c| {
            c.execute("UPDATE setting SET value = '{\"theme\":\"light\",\"future\":1}' WHERE key='app'", [])?;
            Ok(())
        })
        .unwrap();
        let s = load(&db).unwrap();
        assert_eq!(s.theme, "light");
        assert_eq!(s.reader.mode, "single");
        assert!(s.provider("anthropic").is_some());
    }
}
