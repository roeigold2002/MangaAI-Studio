//! Context-aware translation of page batches: builds the request from OCR
//! regions, glossary, translation memory and previous dialogue; calls the
//! provider with retries; validates the JSON before anything is stored.

pub mod memory;
pub mod prompt;
pub mod validate;

use std::sync::Arc;
use std::time::Duration;

use crate::providers::{ErrorKind, ImagePart, ProviderError, ProviderRequest, ProviderResponse, TranslationProvider};
use validate::{Validated, TranslationOut};

#[derive(Debug, Clone)]
pub struct RegionInput {
    pub id: String,
    pub text: String,
    pub kind: String,
    pub vertical: bool,
    pub uncertain: bool,
    /// No OCR text: the model reads the text inside this area of the page image
    /// (percent of width/height: x1, y1, x2, y2).
    pub area: Option<[u32; 4]>,
}

#[derive(Debug, Clone)]
pub struct PageInput {
    pub page_no: i64,
    pub regions: Vec<RegionInput>,
    pub image: Option<ImagePart>,
}

#[derive(Debug, Clone, Default)]
pub struct Context {
    pub series_title: String,
    pub series_notes: String,
    /// Confirmed glossary lines: "source → target (kind, gender) note"
    pub glossary: Vec<String>,
    /// Earlier dialogue in this chapter: "[speaker] source → translation"
    pub previous_lines: Vec<String>,
    pub previous_summaries: Vec<String>,
    /// Approved translations for identical source lines.
    pub memory_hints: Vec<String>,
}

pub fn user_message(ctx: &Context, pages: &[PageInput]) -> String {
    let mut s = String::new();
    s.push_str(&format!("Series: {}\n", ctx.series_title));
    if !ctx.series_notes.trim().is_empty() {
        s.push_str(&format!("\nTranslator notes for this series (follow them):\n{}\n", ctx.series_notes.trim()));
    }
    if !ctx.glossary.is_empty() {
        s.push_str("\nGlossary (use exactly):\n");
        for g in &ctx.glossary {
            s.push_str(&format!("- {g}\n"));
        }
    }
    if !ctx.memory_hints.is_empty() {
        s.push_str("\nPreviously approved translations of identical lines:\n");
        for m in &ctx.memory_hints {
            s.push_str(&format!("- {m}\n"));
        }
    }
    if !ctx.previous_summaries.is_empty() {
        s.push_str("\nStory so far (earlier pages):\n");
        for m in &ctx.previous_summaries {
            s.push_str(&format!("- {m}\n"));
        }
    }
    if !ctx.previous_lines.is_empty() {
        s.push_str("\nPrevious lines (already translated, for continuity):\n");
        for l in &ctx.previous_lines {
            s.push_str(&format!("{l}\n"));
        }
    }
    let with_images = pages.iter().filter(|p| p.image.is_some()).count();
    if with_images > 0 {
        s.push_str(&format!("\nThe {with_images} attached image(s) are these pages, in order.\n"));
    }
    s.push_str("\nTranslate these items (reading order):\n");
    for p in pages {
        s.push_str(&format!("\n## Page {}\n", p.page_no));
        for r in &p.regions {
            let mut flags = vec![r.kind.as_str()];
            if r.vertical {
                flags.push("vertical");
            }
            if r.uncertain {
                flags.push("OCR uncertain");
            }
            match r.area {
                Some([x1, y1, x2, y2]) => s.push_str(&format!(
                    "[{}] ({}, no OCR: read the text in the page image at x {x1}–{x2}%, y {y1}–{y2}%)\n",
                    r.id,
                    flags.join(", ")
                )),
                None => s.push_str(&format!("[{}] ({}) {}\n", r.id, flags.join(", "), r.text.replace('\n', " "))),
            }
        }
    }
    s
}

#[derive(Debug)]
pub struct BatchResult {
    pub validated: Validated,
    pub responses: Vec<ProviderResponse>,
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl RetryPolicy {
    pub fn delay(&self, attempt: u32, err: Option<&ProviderError>) -> Duration {
        if let Some(d) = err.and_then(|e| e.retry_after) {
            return d.min(Duration::from_secs(120));
        }
        let exp = self.base_delay.saturating_mul(1 << attempt.min(6));
        exp.min(Duration::from_secs(60))
    }
}

/// Translate a batch. Retries transport errors and malformed output, then
/// re-asks only for ids the model skipped. Returns whatever was validated;
/// ids still missing are reported in `validated.missing`.
pub async fn translate_batch(
    provider: Arc<dyn TranslationProvider>,
    system: &str,
    ctx: &Context,
    pages: &[PageInput],
    max_output_tokens: u32,
    effort: &str,
    policy: &RetryPolicy,
) -> Result<BatchResult, ProviderError> {
    let expected: Vec<String> = pages.iter().flat_map(|p| p.regions.iter().map(|r| r.id.clone())).collect();
    let images: Vec<ImagePart> = pages.iter().filter_map(|p| p.image.clone()).collect();
    let mut req = ProviderRequest {
        system: system.to_string(),
        user: user_message(ctx, pages),
        images,
        schema: prompt::schema(),
        max_output_tokens,
        effort: effort.to_string(),
    };
    let mut merged: Option<Validated> = None;
    let mut responses = vec![];
    let mut attempt = 0u32;
    let mut want = expected.clone();
    loop {
        let res = provider.complete(&req).await;
        match res {
            Err(e) if e.retryable() && attempt < policy.max_retries => {
                tracing::warn!(attempt, kind = ?e.kind, "provider error, retrying: {}", e.message);
                if e.kind == ErrorKind::Truncated {
                    req.max_output_tokens = (req.max_output_tokens * 2).min(32000);
                }
                tokio::time::sleep(policy.delay(attempt, Some(&e))).await;
                attempt += 1;
                continue;
            }
            Err(e) => {
                if let Some(v) = merged {
                    // keep what we already have; caller marks the rest failed
                    return Ok(BatchResult { validated: v, responses, attempts: attempt + 1 });
                }
                return Err(e);
            }
            Ok(resp) => {
                let parsed = validate::validate(&resp.text, &want);
                responses.push(resp);
                match parsed {
                    Err(why) => {
                        if attempt >= policy.max_retries {
                            if let Some(v) = merged {
                                return Ok(BatchResult { validated: v, responses, attempts: attempt + 1 });
                            }
                            return Err(ProviderError::new(ErrorKind::Server, format!("model output was invalid: {why}")));
                        }
                        tracing::warn!(attempt, "invalid output, retrying: {why}");
                        req.user = format!(
                            "{}\n\nYour previous answer was rejected: {why}. Reply with JSON only, matching the schema.",
                            user_message(ctx, pages)
                        );
                        attempt += 1;
                        continue;
                    }
                    Ok(v) => {
                        let mut m = match merged.take() {
                            None => v,
                            Some(mut prev) => {
                                prev.out.translations.extend(v.out.translations);
                                prev.out.glossary_suggestions.extend(v.out.glossary_suggestions);
                                if prev.out.page_notes.is_empty() {
                                    prev.out.page_notes = v.out.page_notes;
                                }
                                prev.warnings.extend(v.warnings);
                                prev.missing = v.missing;
                                prev
                            }
                        };
                        m.missing = expected
                            .iter()
                            .filter(|id| !m.out.translations.iter().any(|t| &t.id == *id))
                            .cloned()
                            .collect();
                        if m.missing.is_empty() || attempt >= policy.max_retries {
                            return Ok(BatchResult { validated: m, responses, attempts: attempt + 1 });
                        }
                        // ask again only for the skipped items
                        want = m.missing.clone();
                        let sub: Vec<PageInput> = pages
                            .iter()
                            .map(|p| PageInput {
                                page_no: p.page_no,
                                regions: p.regions.iter().filter(|r| want.contains(&r.id)).cloned().collect(),
                                image: p.image.clone(),
                            })
                            .filter(|p| !p.regions.is_empty())
                            .collect();
                        req.images = sub.iter().filter_map(|p| p.image.clone()).collect();
                        req.user = format!(
                            "{}\n\nYou previously skipped these ids; translate them now.",
                            user_message(ctx, &sub)
                        );
                        merged = Some(m);
                        attempt += 1;
                    }
                }
            }
        }
    }
}

pub fn empty_out() -> TranslationOut {
    TranslationOut { translations: vec![], glossary_suggestions: vec![], page_notes: String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::test_support::MockProvider;

    fn page(ids: &[&str]) -> PageInput {
        PageInput {
            page_no: 1,
            regions: ids
                .iter()
                .map(|i| RegionInput { id: i.to_string(), text: "テスト".into(), kind: "bubble".into(), vertical: true, uncertain: false, area: None })
                .collect(),
            image: None,
        }
    }

    fn ok(items: &[(&str, &str)]) -> Result<String, ProviderError> {
        let tr: Vec<serde_json::Value> = items
            .iter()
            .map(|(id, t)| serde_json::json!({"id": id, "translation": t, "category": "dialogue", "speaker": "", "confidence": "high", "note": ""}))
            .collect();
        Ok(serde_json::json!({"translations": tr, "glossary_suggestions": [], "page_notes": "n"}).to_string())
    }

    fn policy() -> RetryPolicy {
        RetryPolicy { max_retries: 3, base_delay: Duration::from_millis(1) }
    }

    #[tokio::test]
    async fn retries_transport_then_malformed_then_succeeds() {
        let p = MockProvider::new(vec![
            Err(ProviderError::new(ErrorKind::RateLimited, "slow down")),
            Ok("not json at all".into()),
            ok(&[("p1r1", "א"), ("p1r2", "ב")]),
        ]);
        let r = translate_batch(p.clone(), "sys", &Context::default(), &[page(&["p1r1", "p1r2"])], 1000, "medium", &policy())
            .await
            .unwrap();
        assert!(r.validated.missing.is_empty());
        assert_eq!(r.attempts, 3);
        assert!(p.requests.lock()[2].user.contains("rejected"));
    }

    #[tokio::test]
    async fn re_asks_only_missing_ids() {
        let p = MockProvider::new(vec![ok(&[("p1r1", "א")]), ok(&[("p1r2", "ב")])]);
        let r = translate_batch(p.clone(), "sys", &Context::default(), &[page(&["p1r1", "p1r2"])], 1000, "medium", &policy())
            .await
            .unwrap();
        assert!(r.validated.missing.is_empty());
        assert_eq!(r.validated.out.translations.len(), 2);
        let second = &p.requests.lock()[1].user;
        assert!(second.contains("[p1r2]") && !second.contains("[p1r1]"));
    }

    #[tokio::test]
    async fn auth_error_is_not_retried() {
        let p = MockProvider::new(vec![Err(ProviderError::new(ErrorKind::Auth, "bad key")), ok(&[("p1r1", "א")])]);
        let e = translate_batch(p.clone(), "s", &Context::default(), &[page(&["p1r1"])], 1000, "low", &policy()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Auth);
        assert_eq!(p.requests.lock().len(), 1);
    }

    #[tokio::test]
    async fn gives_up_after_max_retries_with_partial_result() {
        let p = MockProvider::new(vec![ok(&[("p1r1", "א")]), Ok("garbage".into()), Ok("garbage".into()), Ok("garbage".into())]);
        let r = translate_batch(p, "s", &Context::default(), &[page(&["p1r1", "p1r2"])], 1000, "low", &policy()).await.unwrap();
        assert_eq!(r.validated.missing, vec!["p1r2"]);
    }

    #[test]
    fn message_contains_context_and_flags() {
        let ctx = Context {
            series_title: "Test".into(),
            glossary: vec!["田中 → טנאקה (character, m)".into()],
            previous_lines: vec!["[Tanaka] おはよう → בוקר טוב".into()],
            ..Default::default()
        };
        let mut pg = page(&["p3r1"]);
        pg.regions[0].uncertain = true;
        let m = user_message(&ctx, &[pg]);
        assert!(m.contains("טנאקה") && m.contains("בוקר טוב") && m.contains("OCR uncertain") && m.contains("[p3r1]"));
    }

    #[test]
    fn backoff_is_capped_and_honors_retry_after() {
        let p = RetryPolicy { max_retries: 5, base_delay: Duration::from_secs(2) };
        assert_eq!(p.delay(0, None), Duration::from_secs(2));
        assert_eq!(p.delay(10, None), Duration::from_secs(60));
        let mut e = ProviderError::new(ErrorKind::RateLimited, "");
        e.retry_after = Some(Duration::from_secs(7));
        assert_eq!(p.delay(3, Some(&e)), Duration::from_secs(7));
    }
}
