//! Natural ("human") ordering for page and chapter names: `2.jpg` < `10.jpg`,
//! `page_002` < `page_010`, compared path component by component.

use std::cmp::Ordering;

#[derive(Debug, PartialEq)]
enum Chunk<'a> {
    Num(&'a str),
    Text(&'a str),
}

fn chunks(s: &str) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_digit: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let d = c.is_ascii_digit();
        match in_digit {
            Some(prev) if prev != d => {
                out.push(if prev { Chunk::Num(&s[start..i]) } else { Chunk::Text(&s[start..i]) });
                start = i;
            }
            _ => {}
        }
        in_digit = Some(d);
    }
    if let Some(d) = in_digit {
        out.push(if d { Chunk::Num(&s[start..]) } else { Chunk::Text(&s[start..]) });
    }
    out
}

fn cmp_num(a: &str, b: &str) -> Ordering {
    let ta = a.trim_start_matches('0');
    let tb = b.trim_start_matches('0');
    ta.len()
        .cmp(&tb.len())
        .then_with(|| ta.cmp(tb))
        // equal value: fewer leading zeros first, so the order is total and stable
        .then_with(|| a.len().cmp(&b.len()))
}

/// Compare a single name (no separators).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let o = match (x, y) {
            (Chunk::Num(p), Chunk::Num(q)) => cmp_num(p, q),
            (Chunk::Text(p), Chunk::Text(q)) => {
                let lp = p.to_lowercase();
                let lq = q.to_lowercase();
                lp.cmp(&lq)
            }
            // numbers sort before text ("1a" < "a1")
            (Chunk::Num(_), Chunk::Text(_)) => Ordering::Less,
            (Chunk::Text(_), Chunk::Num(_)) => Ordering::Greater,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len()).then_with(|| a.cmp(b))
}

/// Compare relative paths component by component (`/` or `\` separated).
pub fn natural_path_cmp(a: &str, b: &str) -> Ordering {
    let pa: Vec<&str> = a.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let pb: Vec<&str> = b.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    for (x, y) in pa.iter().zip(pb.iter()) {
        let o = natural_cmp(x, y);
        if o != Ordering::Equal {
            return o;
        }
    }
    pa.len().cmp(&pb.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_path_cmp(a, b));
        v
    }

    #[test]
    fn numeric_not_lexicographic() {
        assert_eq!(sorted(vec!["10.jpg", "2.jpg", "1.jpg", "02.jpg"]), vec!["1.jpg", "2.jpg", "02.jpg", "10.jpg"]);
    }

    #[test]
    fn prefixed_names() {
        assert_eq!(
            sorted(vec!["page_010.png", "page_001.png", "page_002.png", "Page_003.png"]),
            vec!["page_001.png", "page_002.png", "Page_003.png", "page_010.png"]
        );
    }

    #[test]
    fn nested_chapter_paths() {
        assert_eq!(
            sorted(vec!["chapter-10/page-001.webp", "chapter-2/page-010.webp", "chapter-2/page-002.webp", "chapter-01/page-001.webp"]),
            vec!["chapter-01/page-001.webp", "chapter-2/page-002.webp", "chapter-2/page-010.webp", "chapter-10/page-001.webp"]
        );
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(natural_cmp("p99999999999999999999999", "p100000000000000000000000"), Ordering::Less);
    }

    #[test]
    fn non_ascii_names() {
        assert_eq!(sorted(vec!["第10話", "第2話", "第1話"]), vec!["第1話", "第2話", "第10話"]);
    }
}
