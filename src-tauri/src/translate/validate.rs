//! Parse and validate provider output before anything is written.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::prompt::{CATEGORIES, GLOSSARY_KINDS};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemOut {
    pub id: String,
    pub translation: String,
    pub category: String,
    #[serde(default)]
    pub speaker: String,
    pub confidence: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlossaryOut {
    pub source: String,
    pub target: String,
    pub kind: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranslationOut {
    pub translations: Vec<ItemOut>,
    #[serde(default)]
    pub glossary_suggestions: Vec<GlossaryOut>,
    #[serde(default)]
    pub page_notes: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Validated {
    pub out: TranslationOut,
    /// Expected ids the model did not return.
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

/// Strip ```json fences or leading prose some local models add.
fn extract_json(raw: &str) -> &str {
    let t = raw.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    match (t.find('{'), t.rfind('}')) {
        (Some(a), Some(b)) if b > a => &t[a..=b],
        _ => t,
    }
}

pub fn validate(raw: &str, expected: &[String]) -> Result<Validated, String> {
    let mut out: TranslationOut =
        serde_json::from_str(extract_json(raw)).map_err(|e| format!("output is not valid JSON for the schema: {e}"))?;
    let expected_set: HashSet<&str> = expected.iter().map(|s| s.as_str()).collect();
    let mut warnings = vec![];
    let mut seen = HashSet::new();
    out.translations.retain(|it| {
        if !expected_set.contains(it.id.as_str()) {
            warnings.push(format!("ignored unknown id {}", it.id));
            return false;
        }
        if !seen.insert(it.id.clone()) {
            warnings.push(format!("ignored duplicate id {}", it.id));
            return false;
        }
        true
    });
    for it in out.translations.iter_mut() {
        it.translation = it.translation.trim().to_string();
        if !CATEGORIES.contains(&it.category.as_str()) {
            it.category = "other".into();
        }
        if !matches!(it.confidence.as_str(), "high" | "medium" | "low") {
            it.confidence = "medium".into();
        }
        if it.translation.is_empty() {
            it.confidence = "low".into();
            if it.note.is_empty() {
                it.note = "empty translation".into();
            }
        }
    }
    out.glossary_suggestions.retain(|g| !g.source.trim().is_empty() && !g.target.trim().is_empty());
    for g in out.glossary_suggestions.iter_mut() {
        if !GLOSSARY_KINDS.contains(&g.kind.as_str()) {
            g.kind = "other".into();
        }
        if !matches!(g.gender.as_str(), "m" | "f" | "n") {
            g.gender.clear();
        }
    }
    let missing: Vec<String> = expected.iter().filter(|id| !seen.contains(*id)).cloned().collect();
    Ok(Validated { out, missing, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_valid_output() {
        let raw = r#"{"translations":[{"id":"p1r1","translation":"שלום","category":"dialogue","speaker":"","confidence":"high","note":""}],
                     "glossary_suggestions":[],"page_notes":"x"}"#;
        let v = validate(raw, &ids(&["p1r1"])).unwrap();
        assert!(v.missing.is_empty());
        assert_eq!(v.out.translations[0].translation, "שלום");
    }

    #[test]
    fn reports_missing_and_drops_unknown_and_duplicates() {
        let raw = r#"```json
        {"translations":[
          {"id":"p1r1","translation":"א","category":"dialogue","speaker":"","confidence":"high","note":""},
          {"id":"p1r1","translation":"ב","category":"dialogue","speaker":"","confidence":"high","note":""},
          {"id":"zzz","translation":"ג","category":"weird","speaker":"","confidence":"sure","note":""}
        ],"glossary_suggestions":[{"source":"","target":"x","kind":"character","gender":"m","note":""}],"page_notes":""}
        ```"#;
        let v = validate(raw, &ids(&["p1r1", "p1r2"])).unwrap();
        assert_eq!(v.missing, vec!["p1r2"]);
        assert_eq!(v.out.translations.len(), 1);
        assert_eq!(v.out.translations[0].translation, "א");
        assert_eq!(v.warnings.len(), 2);
        assert!(v.out.glossary_suggestions.is_empty());
    }

    #[test]
    fn normalises_bad_enums_and_empty_text() {
        let raw = r#"{"translations":[{"id":"a","translation":"  ","category":"weird","speaker":"","confidence":"sure","note":""}],"glossary_suggestions":[],"page_notes":""}"#;
        let v = validate(raw, &ids(&["a"])).unwrap();
        let t = &v.out.translations[0];
        assert_eq!((t.category.as_str(), t.confidence.as_str()), ("other", "low"));
    }

    #[test]
    fn rejects_malformed() {
        assert!(validate("I cannot help with that", &ids(&["a"])).is_err());
        assert!(validate(r#"{"translations": "nope"}"#, &ids(&["a"])).is_err());
        assert!(validate(r#"{"translations":[{"id":"a"}]}"#, &ids(&["a"])).is_err());
    }
}
