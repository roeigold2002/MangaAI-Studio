//! Discover manga/chapter/page structure on disk without modifying anything.

use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

use super::natsort::{natural_cmp, natural_path_cmp};
use crate::error::{AppError, AppResult};

pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif"];
pub const ARCHIVE_EXTS: &[&str] = &["cbz", "zip"];
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Container {
    Folder,
    Archive,
}

impl Container {
    pub fn as_str(&self) -> &'static str {
        match self {
            Container::Folder => "folder",
            Container::Archive => "archive",
        }
    }
    pub fn parse(s: &str) -> Self {
        if s == "archive" { Container::Archive } else { Container::Folder }
    }
}

#[derive(Debug, Clone)]
pub struct ChapterSource {
    pub container: Container,
    /// Folder path, or archive file path.
    pub location: PathBuf,
    /// Directory inside the archive ("" for root). Always "" for folders.
    pub inner_prefix: String,
    pub title: String,
    pub number: Option<f64>,
    pub volume: Option<f64>,
    /// Absolute file paths (folder) or entry names (archive), naturally sorted.
    pub pages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MangaSource {
    pub title: String,
    pub root: PathBuf,
    pub chapters: Vec<ChapterSource>,
    pub cover: Option<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn is_image_name(name: &str) -> bool {
    IMAGE_EXTS.contains(&ext_of(name).as_str())
}

pub fn is_archive_name(name: &str) -> bool {
    ARCHIVE_EXTS.contains(&ext_of(name).as_str())
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.') || name == "__MACOSX" || name == "Thumbs.db"
}

fn is_cover_name(stem: &str) -> bool {
    matches!(stem.to_ascii_lowercase().as_str(), "cover" | "folder" | "poster")
}

static NUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:(?:^|[^a-z])(?:ch(?:apter|ap)?|ep(?:isode)?)\.?|第|#|話)\s*[._-]?\s*(\d+(?:\.\d+)?)").unwrap()
});
static VOL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[^a-z])v(?:ol(?:ume)?)?\.?\s*[._-]?\s*(\d+(?:\.\d+)?)|(\d+(?:\.\d+)?)\s*巻").unwrap()
});
static LAST_NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)").unwrap());

/// Best-effort chapter/volume numbers from a name like "Vol.02 Ch.010.5" or "v04".
pub fn parse_numbers(name: &str) -> (Option<f64>, Option<f64>) {
    let chapter = NUM_RE.captures(name).and_then(|c| c[1].parse().ok());
    let volume = VOL_RE
        .captures(name)
        .and_then(|c| c.get(1).or_else(|| c.get(2)).and_then(|m| m.as_str().parse().ok()));
    let chapter = chapter.or_else(|| {
        if volume.is_some() {
            None
        } else {
            LAST_NUM_RE.captures_iter(name).last().and_then(|c| c[1].parse().ok())
        }
    });
    (chapter, volume)
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// List image entries of a ZIP/CBZ grouped by their directory inside the archive.
pub fn archive_groups(path: &Path) -> AppResult<BTreeMap<String, Vec<String>>> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let parts: Vec<&str> = name.split('/').collect();
        if parts.iter().any(|p| is_hidden(p)) || !is_image_name(&name) {
            continue;
        }
        let prefix = match name.rfind('/') {
            Some(i) => name[..i].to_string(),
            None => String::new(),
        };
        groups.entry(prefix).or_default().push(name);
    }
    for v in groups.values_mut() {
        v.sort_by(|a, b| natural_path_cmp(a, b));
    }
    Ok(groups)
}

fn archive_chapters(path: &Path, base_title: &str) -> AppResult<Vec<ChapterSource>> {
    let groups = archive_groups(path)?;
    let multiple = groups.len() > 1;
    let mut out: Vec<ChapterSource> = groups
        .into_iter()
        .filter(|(_, pages)| !pages.is_empty())
        .map(|(prefix, pages)| {
            let title = if multiple && !prefix.is_empty() {
                prefix.rsplit('/').next().unwrap_or(&prefix).to_string()
            } else {
                base_title.to_string()
            };
            let (number, volume) = parse_numbers(&title);
            ChapterSource {
                container: Container::Archive,
                location: path.to_path_buf(),
                inner_prefix: prefix,
                title,
                number,
                volume,
                pages,
            }
        })
        .collect();
    out.sort_by(|a, b| natural_path_cmp(&a.inner_prefix, &b.inner_prefix));
    Ok(out)
}

/// Scan one manga: a folder (chapters = leaf folders with images, or archives),
/// or a single archive.
pub fn scan_manga(root: &Path) -> AppResult<MangaSource> {
    let meta = std::fs::metadata(root)
        .map_err(|e| AppError::new("not_found", format!("{}: {e}", root.display())))?;
    let mut warnings = Vec::new();

    if meta.is_file() {
        let name = name_of(root);
        if !is_archive_name(&name) {
            return Err(AppError::invalid(format!(
                "unsupported file type: {name} (supported: folders, .cbz, .zip)"
            )));
        }
        let stem = stem_of(root);
        let chapters = archive_chapters(root, &stem)?;
        if chapters.is_empty() {
            return Err(AppError::new("empty", format!("no images found in {name}")));
        }
        return Ok(MangaSource { title: stem, root: root.to_path_buf(), chapters, cover: None, warnings });
    }

    // Folder: group images by parent directory, collect archives.
    let mut by_dir: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    let mut archives: Vec<PathBuf> = Vec::new();
    let walker = WalkDir::new(root)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(&e.file_name().to_string_lossy()));
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("skipped unreadable entry: {e}"));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_image_name(&name) {
            let parent = entry.path().parent().unwrap_or(root).to_path_buf();
            by_dir.entry(parent).or_default().push(entry.path().to_path_buf());
        } else if is_archive_name(&name) {
            archives.push(entry.path().to_path_buf());
        } else if ext_of(&name) == "cbr" || ext_of(&name) == "rar" {
            warnings.push(format!("RAR/CBR is not supported, extract it first: {name}"));
        }
    }

    // A lone cover image at the root next to chapters is a cover, not a chapter.
    let mut cover = None;
    if let Some(root_imgs) = by_dir.get(root) {
        let has_other = by_dir.len() > 1 || !archives.is_empty();
        if has_other && root_imgs.len() <= 2 {
            cover = root_imgs.iter().find(|p| is_cover_name(&stem_of(p))).cloned().or_else(|| {
                if root_imgs.len() == 1 { root_imgs.first().cloned() } else { None }
            });
            if cover.is_some() {
                by_dir.remove(root);
            }
        }
    }

    let rel = |p: &Path| -> String {
        p.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default()
    };

    let mut chapters: Vec<(String, ChapterSource)> = Vec::new();
    for (dir, mut files) in by_dir {
        files.sort_by(|a, b| natural_cmp(&name_of(a), &name_of(b)));
        let r = rel(&dir);
        let title = if r.is_empty() { name_of(root) } else { r.replace('/', " · ") };
        let (number, volume) = parse_numbers(&name_of(&dir));
        chapters.push((
            r,
            ChapterSource {
                container: Container::Folder,
                location: dir,
                inner_prefix: String::new(),
                title,
                number,
                volume,
                pages: files.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            },
        ));
    }
    for arc in archives {
        let r = rel(&arc);
        match archive_chapters(&arc, &stem_of(&arc)) {
            Ok(list) => {
                for ch in list {
                    let key = format!("{r}/{}", ch.inner_prefix);
                    chapters.push((key, ch));
                }
            }
            Err(e) => warnings.push(format!("{}: {}", name_of(&arc), e.message)),
        }
    }
    chapters.sort_by(|a, b| natural_path_cmp(&a.0, &b.0));
    let chapters: Vec<ChapterSource> = chapters.into_iter().map(|(_, c)| c).collect();
    if chapters.is_empty() {
        return Err(AppError::new("empty", format!("no images or CBZ/ZIP archives found in {}", root.display())));
    }
    Ok(MangaSource { title: name_of(root), root: root.to_path_buf(), chapters, cover, warnings })
}

/// Scan a library root: each sub-folder or archive directly inside is one manga.
pub fn scan_library_root(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden(&name) {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() || (ft.is_file() && is_archive_name(&name)) {
            out.push(entry.path());
        }
    }
    out.sort_by(|a, b| natural_cmp(&name_of(a), &name_of(b)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    pub fn make_zip(path: &Path, entries: &[&str]) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for e in entries {
            z.start_file(*e, opts).unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn single_folder_is_single_chapter_in_numeric_order() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("My Manga");
        for n in ["10.jpg", "2.jpg", "1.jpg", "notes.txt", ".hidden.jpg"] {
            touch(&root.join(n));
        }
        let m = scan_manga(&root).unwrap();
        assert_eq!(m.title, "My Manga");
        assert_eq!(m.chapters.len(), 1);
        let names: Vec<String> = m.chapters[0].pages.iter().map(|p| name_of(Path::new(p))).collect();
        assert_eq!(names, vec!["1.jpg", "2.jpg", "10.jpg"]);
    }

    #[test]
    fn chapter_folders_and_cover() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Series");
        touch(&root.join("cover.jpg"));
        touch(&root.join("chapter-10/page-001.webp"));
        touch(&root.join("chapter-2/page-002.webp"));
        touch(&root.join("chapter-2/page-001.webp"));
        touch(&root.join("Vol 1/Ch 1/001.png"));
        let m = scan_manga(&root).unwrap();
        assert!(m.cover.is_some());
        let titles: Vec<&str> = m.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["chapter-2", "chapter-10", "Vol 1 · Ch 1"]);
        assert_eq!(m.chapters[0].number, Some(2.0));
        assert_eq!(m.chapters[0].pages.len(), 2);
        assert!(m.chapters[0].pages[0].ends_with("page-001.webp"));
    }

    #[test]
    fn archives_as_chapters_and_nested_dirs_inside_zip() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Zips");
        std::fs::create_dir_all(&root).unwrap();
        make_zip(&root.join("Vol.02.cbz"), &["10.jpg", "2.jpg", "1.jpg"]);
        make_zip(&root.join("Vol.01.cbz"), &["a/1.jpg", "b/1.jpg", "b/2.png", "__MACOSX/a/._1.jpg"]);
        let m = scan_manga(&root).unwrap();
        let titles: Vec<&str> = m.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["a", "b", "Vol.02"]);
        assert_eq!(m.chapters[2].pages, vec!["1.jpg", "2.jpg", "10.jpg"]);
        assert_eq!(m.chapters[1].inner_prefix, "b");
        assert_eq!(m.chapters[2].volume, Some(2.0));
    }

    #[test]
    fn single_cbz_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("One Shot.cbz");
        make_zip(&p, &["p1.jpg", "p2.jpg"]);
        let m = scan_manga(&p).unwrap();
        assert_eq!(m.title, "One Shot");
        assert_eq!(m.chapters.len(), 1);
        assert_eq!(m.chapters[0].container, Container::Archive);
    }

    #[test]
    fn corrupted_archive_is_reported_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Mixed");
        touch(&root.join("ch1/1.jpg"));
        std::fs::write(root.join("broken.cbz"), b"not a zip").unwrap();
        let m = scan_manga(&root).unwrap();
        assert_eq!(m.chapters.len(), 1);
        assert!(m.warnings.iter().any(|w| w.contains("broken.cbz")));
    }

    #[test]
    fn empty_folder_errors() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(scan_manga(d.path()).unwrap_err().code, "empty");
    }

    #[test]
    fn number_parsing() {
        assert_eq!(parse_numbers("Chapter 12.5"), (Some(12.5), None));
        assert_eq!(parse_numbers("Vol.03 Ch.021"), (Some(21.0), Some(3.0)));
        assert_eq!(parse_numbers("DLRAW.AC_Kuso Onna ni Sachi Are v04"), (None, Some(4.0)));
        assert_eq!(parse_numbers("第5話"), (Some(5.0), None));
        assert_eq!(parse_numbers("042"), (Some(42.0), None));
    }
}
