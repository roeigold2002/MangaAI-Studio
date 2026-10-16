//! Choosing one version per chapter. Multi-scanlator sources (MangaDex, MangaFire…)
//! list every chapter once per group, and officially licensed chapters often appear
//! as a single "external chapter, read it on the publisher's site" image. Bulk
//! downloads keep one version per chapter number and leave those placeholders out.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::sources::RemoteChapter;

/// Grouping key: the chapter number (104 and 104.5 are different chapters), or the
/// title when the source gives no number.
pub fn chapter_key(number: Option<f64>, title: &str) -> String {
    match number {
        Some(n) => format!("n:{}", (n * 1000.0).round() as i64),
        None => format!("t:{}", title.trim().to_lowercase()),
    }
}

/// Decimal chapters (12.5, 104.1) and titled extras are legitimately short: a single
/// illustration page is real content there.
fn is_extra(number: Option<f64>, title: &str) -> bool {
    if number.map_or(true, |n| n.fract().abs() > 1e-6) {
        return true;
    }
    let t = title.to_lowercase();
    ["illust", "extra", "bonus", "omake", "cover", "artwork", "special", "color page", "colour page"]
        .iter()
        .any(|w| t.contains(w))
}

/// Whether a chapter with this many pages is a stand-in rather than the chapter: a
/// regular (whole-number) chapter that has only one page.
pub fn is_placeholder(number: Option<f64>, title: &str, pages: Option<i64>) -> bool {
    matches!(pages, Some(p) if p <= 1) && !is_extra(number, title)
}

fn same_group(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if a.trim().eq_ignore_ascii_case(b.trim()))
}

/// Higher is better: versions known to be one page lose to any other version, then
/// the preferred group wins, then more pages, then the newest upload.
fn rank(c: &RemoteChapter, preferred: Option<&str>) -> (bool, bool, i64, i64) {
    (
        !matches!(c.page_count, Some(p) if p <= 1),
        same_group(c.scanlator.as_deref(), preferred),
        c.page_count.unwrap_or(0),
        c.upload_date.unwrap_or(0),
    )
}

/// The scanlation group to prefer: the one most of the existing downloads came from.
pub fn most_common_group<'a>(groups: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<String, (usize, String)> = HashMap::new();
    for g in groups.into_iter().map(str::trim).filter(|g| !g.is_empty()) {
        counts.entry(g.to_lowercase()).or_insert((0, g.to_string())).0 += 1;
    }
    let mut v: Vec<_> = counts.into_values().collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    match v.as_slice() {
        [first, second, ..] if first.0 == second.0 => None, // a tie says nothing
        [first, ..] => Some(first.1.clone()),
        [] => None,
    }
}

#[derive(Debug, Clone)]
pub struct Pick {
    pub chapter: RemoteChapter,
    /// Other versions of the same chapter, best first: used when the pick turns out
    /// to be a placeholder once its pages are listed.
    pub alternates: Vec<RemoteChapter>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PlanSummary {
    /// Chapters that will be queued (one version each).
    pub chapters: usize,
    /// Extra versions of those chapters that were left out.
    pub duplicates: usize,
    /// Chapters whose only versions are one-page placeholders.
    pub placeholders: usize,
    /// Chapters already downloaded or in the queue (any version).
    pub already: usize,
    pub preferred: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub picks: Vec<Pick>,
    pub summary: PlanSummary,
}

/// One version per chapter number out of `chapters` (in their original order).
/// `have` holds the keys of chapters already downloaded or queued.
pub fn plan(chapters: &[RemoteChapter], have: &HashSet<String>, preferred: Option<&str>) -> Plan {
    let mut order: Vec<String> = vec![];
    let mut groups: HashMap<String, Vec<&RemoteChapter>> = HashMap::new();
    for c in chapters {
        let k = chapter_key(c.number, &c.title);
        if !groups.contains_key(&k) {
            order.push(k.clone());
        }
        groups.entry(k).or_default().push(c);
    }
    let mut out = Plan { picks: vec![], summary: PlanSummary { preferred: preferred.map(String::from), ..Default::default() } };
    for k in order {
        let mut versions = groups.remove(&k).unwrap_or_default();
        if have.contains(&k) {
            out.summary.already += 1;
            continue;
        }
        versions.sort_by(|a, b| rank(b, preferred).cmp(&rank(a, preferred)));
        let best = versions[0];
        if is_placeholder(best.number, &best.title, best.page_count) {
            out.summary.placeholders += 1;
            continue;
        }
        out.summary.duplicates += versions.len() - 1;
        out.picks.push(Pick {
            chapter: best.clone(),
            alternates: versions[1..]
                .iter()
                .filter(|c| !is_placeholder(c.number, &c.title, c.page_count))
                .map(|c| (*c).clone())
                .collect(),
        });
    }
    out.summary.chapters = out.picks.len();
    out
}

/// One chapter (or queued download) of a manga, as seen by the duplicate cleanup.
#[derive(Debug, Clone)]
pub struct Held {
    pub number: Option<f64>,
    pub title: String,
    pub scanlator: Option<String>,
    /// Known page count (downloaded pages, or the source's count when known).
    pub pages: Option<i64>,
    /// Downloaded (in the library) rather than still queued/failed.
    pub done: bool,
    /// Translated pages or a translation job: never removed.
    pub protected: bool,
    /// Tie-breaker: newer entries win.
    pub seq: i64,
}

fn held_rank(h: &Held, preferred: Option<&str>) -> (bool, bool, bool, bool, bool, i64, i64) {
    (
        !is_placeholder(h.number, &h.title, h.pages),
        h.protected,
        h.done,
        !matches!(h.pages, Some(p) if p <= 1),
        same_group(h.scanlator.as_deref(), preferred),
        h.pages.unwrap_or(0),
        h.seq,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Removal {
    /// Another version of the same chapter is kept.
    Duplicate,
    /// A one-page "read it elsewhere" stand-in.
    Placeholder,
}

/// For each item, whether to remove it (and why). Groups by chapter number, keeps the
/// best version of each, and (with `placeholders`) also drops placeholder chapters
/// that have no real version. Protected items are always kept.
pub fn cleanup(items: &[Held], preferred: Option<&str>, placeholders: bool) -> Vec<Option<Removal>> {
    let mut out = vec![None; items.len()];
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, h) in items.iter().enumerate() {
        groups.entry(chapter_key(h.number, &h.title)).or_default().push(i);
    }
    for mut idx in groups.into_values() {
        idx.sort_by(|&a, &b| held_rank(&items[b], preferred).cmp(&held_rank(&items[a], preferred)));
        for &i in &idx[1..] {
            if !items[i].protected {
                out[i] = Some(Removal::Duplicate);
            }
        }
        let keep = &items[idx[0]];
        if placeholders && keep.done && !keep.protected && is_placeholder(keep.number, &keep.title, keep.pages) {
            out[idx[0]] = Some(Removal::Placeholder);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(r: &str, n: Option<f64>, title: &str, group: &str, pages: Option<i64>, up: i64) -> RemoteChapter {
        RemoteChapter {
            chapter_ref: r.into(),
            title: title.into(),
            number: n,
            scanlator: (!group.is_empty()).then(|| group.to_string()),
            upload_date: Some(up),
            page_count: pages,
        }
    }

    fn refs(p: &Plan) -> Vec<&str> {
        p.picks.iter().map(|p| p.chapter.chapter_ref.as_str()).collect()
    }

    /// Shaped after a real MangaFire listing: two groups, official 1-page stubs,
    /// decimal illustration chapters, three copies of one chapter.
    fn listing() -> Vec<RemoteChapter> {
        vec![
            ch("1a", Some(1.0), "Ch. 1", "unofficial", None, 10),
            ch("1b", Some(1.0), "Ch. 1", "official", None, 20),
            ch("85a", Some(85.0), "Ch. 85", "official", Some(17), 100),
            ch("85b", Some(85.0), "Ch. 85", "unofficial", Some(17), 200),
            ch("85c", Some(85.0), "Ch. 85", "official", Some(17), 300),
            ch("103a", Some(103.0), "Ch. 103", "official", Some(15), 400),
            ch("103b", Some(103.0), "Ch. 103", "unofficial", Some(1), 500),
            ch("104a", Some(104.0), "Ch. 104", "official", Some(16), 600),
            ch("104b", Some(104.0), "Ch. 104", "unofficial", Some(1), 700),
            ch("104.1", Some(104.1), "Ch. 104.1", "official", Some(1), 650),
            ch("104.5", Some(104.5), "Ch. 104.5 - ILLUSTRATION", "unofficial", Some(1), 700),
            ch("99", Some(99.0), "Ch. 99", "unofficial", Some(1), 800),
            ch("x", None, "Special: Q&A", "official", None, 900),
        ]
    }

    #[test]
    fn keeps_one_version_per_chapter_number() {
        let p = plan(&listing(), &HashSet::new(), None);
        // Ch. 1: no preference, unknown sizes -> newest; 85: newest of three equal;
        // 103/104: the real chapter beats the 1-page stub regardless of upload date.
        assert_eq!(refs(&p), vec!["1b", "85c", "103a", "104a", "104.1", "104.5", "x"]);
        assert_eq!(p.summary.chapters, 7);
        assert_eq!(p.summary.duplicates, 1 + 2 + 1 + 1);
        assert_eq!(p.summary.placeholders, 1, "Ch. 99 has only a placeholder");
        assert_eq!(p.summary.already, 0);
    }

    #[test]
    fn decimals_are_separate_chapters_and_one_page_extras_are_kept() {
        let chs = vec![
            ch("a", Some(104.0), "Ch. 104", "g", Some(18), 1),
            ch("b", Some(104.5), "Ch. 104.5", "g", Some(1), 1),
            ch("c", Some(104.1), "Ch. 104.1", "g", Some(1), 1),
            ch("d", Some(12.0), "Ch. 12 - Illustration", "g", Some(1), 1),
            ch("e", Some(13.0), "Ch. 13", "g", Some(1), 1),
        ];
        let p = plan(&chs, &HashSet::new(), None);
        assert_eq!(refs(&p), vec!["a", "b", "c", "d"]);
        assert_eq!(p.summary.placeholders, 1);
    }

    #[test]
    fn preferred_group_wins_but_never_with_a_stub() {
        let p = plan(&listing(), &HashSet::new(), Some("Unofficial"));
        let r = refs(&p);
        assert!(r.contains(&"1a") && r.contains(&"85b"), "{r:?}");
        assert!(r.contains(&"103a") && r.contains(&"104a"), "a 1-page stub never wins: {r:?}");
    }

    #[test]
    fn page_count_then_upload_date_break_ties() {
        let chs = vec![
            ch("small-new", Some(5.0), "Ch. 5", "a", Some(10), 9),
            ch("big-old", Some(5.0), "Ch. 5", "b", Some(20), 1),
            ch("same-old", Some(6.0), "Ch. 6", "a", Some(20), 1),
            ch("same-new", Some(6.0), "Ch. 6", "b", Some(20), 2),
        ];
        assert_eq!(refs(&plan(&chs, &HashSet::new(), None)), vec!["big-old", "same-new"]);
    }

    #[test]
    fn chapters_already_held_are_not_queued_again_in_any_version() {
        let have: HashSet<String> = [chapter_key(Some(85.0), ""), chapter_key(Some(1.0), "")].into();
        let p = plan(&listing(), &have, None);
        let r = refs(&p);
        assert!(!r.iter().any(|x| ["1a", "1b", "85a", "85b", "85c"].contains(x)), "{r:?}");
        assert_eq!(p.summary.already, 2);
    }

    #[test]
    fn alternates_are_ranked_and_exclude_known_stubs() {
        let p = plan(&listing(), &HashSet::new(), None);
        let c104 = p.picks.iter().find(|p| p.chapter.chapter_ref == "104a").unwrap();
        assert!(c104.alternates.is_empty(), "the 1-page unofficial copy is not a fallback");
        let c85 = p.picks.iter().find(|p| p.chapter.chapter_ref == "85c").unwrap();
        assert_eq!(c85.alternates.iter().map(|c| c.chapter_ref.as_str()).collect::<Vec<_>>(), vec!["85b", "85a"]);
    }

    #[test]
    fn most_common_group_ignores_ties_and_blanks() {
        assert_eq!(most_common_group(["a", "b", "a", ""]), Some("a".into()));
        assert_eq!(most_common_group(["a", "b"]), None);
        assert_eq!(most_common_group(Vec::<&str>::new()), None);
    }

    fn held(n: f64, title: &str, group: &str, pages: i64, done: bool, protected: bool, seq: i64) -> Held {
        Held { number: Some(n), title: title.into(), scanlator: Some(group.into()), pages: Some(pages), done, protected, seq }
    }

    #[test]
    fn cleanup_keeps_the_best_copy_and_drops_placeholders() {
        let items = vec![
            held(104.0, "Ch. 104", "unofficial", 1, true, false, 5), // stub
            held(104.0, "Ch. 104", "official", 16, true, false, 6),
            held(85.0, "Ch. 85", "official", 17, true, false, 57),
            held(85.0, "Ch. 85", "unofficial", 17, true, false, 58),
            held(85.0, "Ch. 85", "official", 17, true, false, 59),
            held(104.5, "Ch. 104.5 - ILLUSTRATION", "unofficial", 1, true, false, 3),
            held(99.0, "Ch. 99", "unofficial", 1, true, false, 17), // lone stub
            held(98.0, "Ch. 98", "official", 0, false, false, 90), // queued copy of a done chapter
            held(98.0, "Ch. 98", "official", 20, true, false, 20),
        ];
        let r = cleanup(&items, Some("official"), true);
        use Removal::*;
        assert_eq!(r, vec![Some(Duplicate), None, Some(Duplicate), Some(Duplicate), None, None, Some(Placeholder), Some(Duplicate), None]);
        // Without placeholder removal the lone stub stays.
        assert_eq!(cleanup(&items, Some("official"), false)[6], None);
    }

    #[test]
    fn cleanup_never_removes_translated_work() {
        let items = vec![held(7.0, "Ch. 7", "a", 20, true, false, 2), held(7.0, "Ch. 7", "b", 18, true, true, 1)];
        assert_eq!(cleanup(&items, Some("a"), true), vec![Some(Removal::Duplicate), None]);
        let stub = vec![held(8.0, "Ch. 8", "a", 1, true, true, 1)];
        assert_eq!(cleanup(&stub, None, true), vec![None]);
    }
}
