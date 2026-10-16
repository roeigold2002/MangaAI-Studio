//! Versioned translation prompt and output schema. Changing either must bump
//! PROMPT_VERSION so cached translations are recognised as stale.

use serde_json::{json, Value};

pub const PROMPT_VERSION: &str = "he-v2";

pub const CATEGORIES: &[&str] = &["dialogue", "thought", "narration", "sfx", "sign", "other"];
pub const GLOSSARY_KINDS: &[&str] = &["character", "term", "place", "attack", "organization", "honorific", "other"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        // Short fields first: after a long `translations` array models tend to stop without
        // them, and the Claude CLI then rejects the answer and has it written all over again.
        "required": ["page_notes", "glossary_suggestions", "translations"],
        "properties": {
            "translations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "source", "translation", "category", "speaker", "confidence", "note"],
                    "properties": {
                        "id": {"type": "string"},
                        "source": {"type": "string"},
                        "translation": {"type": "string"},
                        "category": {"type": "string", "enum": CATEGORIES},
                        "speaker": {"type": "string"},
                        "confidence": {"type": "string", "enum": ["high", "medium", "low"]},
                        "note": {"type": "string"}
                    }
                }
            },
            "glossary_suggestions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["source", "target", "kind", "gender", "note"],
                    "properties": {
                        "source": {"type": "string"},
                        "target": {"type": "string"},
                        "kind": {"type": "string", "enum": GLOSSARY_KINDS},
                        "gender": {"type": "string", "enum": ["m", "f", "n", ""]},
                        "note": {"type": "string"}
                    }
                }
            },
            "page_notes": {"type": "string"}
        }
    })
}

pub fn language_name(code: &str) -> &str {
    match code {
        "he" => "Hebrew",
        "ja" => "Japanese",
        "ko" => "Korean",
        "zh" => "Chinese",
        "en" => "English",
        other => other,
    }
}

pub fn system_prompt(source_lang: &str, target_lang: &str, sfx_mode: &str) -> String {
    let src = language_name(source_lang);
    let tgt = language_name(target_lang);
    let hebrew = if target_lang == "he" {
        r#"
Hebrew specifics:
- Write natural, contemporary spoken Hebrew as a professional Israeli manga translator would. Not literal, not stiff, no biblical register unless the character speaks that way.
- Get gender agreement right (verbs, adjectives, pronouns, second person את/אתה). Use the glossary gender of characters, the speaker, and the page images. If gender truly cannot be inferred, choose the most likely form and set confidence to "medium" with a note.
- Hebrew without niqqud. Use standard punctuation; keep "?!", "...", "—" where they carry tone.
- Write names in Hebrew letters (e.g. 田中 → טנאקה) unless the glossary says otherwise. Keep honorifics (-san, -kun, -chan, -sama, senpai, sensei) as טנאקה-סאן style only if the series' glossary or notes ask for it; otherwise convey the politeness in Hebrew wording.
- Never reverse letters or insert direction marks; output plain logical-order Unicode text."#
    } else {
        ""
    };
    let sfx = if sfx_mode == "translate" {
        "Translate sound effects (category \"sfx\") with a short, punchy equivalent."
    } else {
        "For sound effects (category \"sfx\") put a short equivalent in `translation`; the app may leave the original art untouched."
    };
    format!(
        r#"You are an expert manga translator from {src} into {tgt}. You receive the OCR text of the speech bubbles and captions of one or more consecutive manga pages, in reading order, each with a stable id. You may also receive the page images.

Goals:
- Translate meaning, tone, character voice, humor and emotion into natural {tgt}. Avoid unnecessary literal translation; keep wordplay when a {tgt} equivalent exists.
- Keep lines short enough to fit the original bubble. Do not add explanations inside dialogue.
- Use the series glossary exactly as given. Keep names and terms consistent with earlier pages.
- Use the context (previous lines, page images, speaker) to decide who speaks and how.
- Classify each item: dialogue, thought, narration, sfx, sign, other.
- OCR can be wrong. If an item looks garbled, translate what is clearly intended. If you cannot tell what it says, do not invent content: give your best short guess, set confidence "low", and explain in `note`.
- {sfx}
{hebrew}

Some items come without OCR text ("no OCR"): read the original text yourself from the page image inside the given area (percent of the page width/height), then translate it. If that area holds no readable text, return empty `source` and `translation`.

Output: JSON only, matching the provided schema.
- `translations`: exactly one entry per input id, same ids, nothing else.
- `source`: the original text of the item as you read it (copy the given OCR text when it was provided).
- `speaker`: character name if you can tell, otherwise "".
- `confidence`: "high" normally; "medium"/"low" when unsure (OCR or meaning). `note`: short reason, or "".
- `glossary_suggestions`: new recurring names/terms worth fixing for the whole series (not already in the glossary), with your {tgt} rendering; [] if none.
- `page_notes`: 1-2 sentences in English summarising what happened (used as context for the next pages)."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_strict_everywhere() {
        fn check(v: &Value) {
            if v["type"] == "object" {
                assert_eq!(v["additionalProperties"], false, "{v}");
                let props: Vec<&String> = v["properties"].as_object().unwrap().keys().collect();
                let req: Vec<&str> = v["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
                assert_eq!(props.len(), req.len(), "all properties required for strict mode");
                for p in v["properties"].as_object().unwrap().values() {
                    check(p);
                }
            }
            if v["type"] == "array" {
                check(&v["items"]);
            }
        }
        check(&schema());
    }

    #[test]
    fn prompt_mentions_gender_for_hebrew_only() {
        assert!(system_prompt("ja", "he", "keep").contains("gender agreement"));
        assert!(!system_prompt("ja", "en", "keep").contains("gender agreement"));
    }
}
