//! File layout, page byte access, hashing, thumbnails and atomic writes.
//! Original files are only ever read.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::library::scan::{ext_of, Container};

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> AppResult<Self> {
        let p = Self { root };
        for d in [p.library_dir(), p.derived_root(), p.thumbs_dir(), p.logs_dir(), p.tmp_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(p)
    }
    pub fn db_path(&self) -> PathBuf {
        self.root.join("library.db")
    }
    /// Pages downloaded from online sources live here (imports stay in place).
    pub fn library_dir(&self) -> PathBuf {
        self.root.join("library")
    }
    pub fn derived_root(&self) -> PathBuf {
        self.root.join("derived")
    }
    pub fn thumbs_dir(&self) -> PathBuf {
        self.root.join("thumbs")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
    /// Directory for files derived from one source image (content addressed).
    pub fn derived_dir(&self, sha256: &str) -> PathBuf {
        self.derived_root().join(&sha256[..2.min(sha256.len())]).join(sha256)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_str(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

pub fn mime_for(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Read the bytes of a page from a folder file or an archive entry.
pub fn read_page_bytes(container: Container, location: &Path, path: &str) -> AppResult<Vec<u8>> {
    match container {
        Container::Folder => Ok(std::fs::read(path)?),
        Container::Archive => {
            let file = std::fs::File::open(location)?;
            let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
            let mut entry = zip.by_name(path)?;
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

pub fn image_dims(bytes: &[u8]) -> (u32, u32) {
    imagesize::blob_size(bytes).map(|s| (s.width as u32, s.height as u32)).unwrap_or((0, 0))
}

/// Write to a temp file in the same directory, fsync, then rename over the target.
pub fn atomic_write(target: &Path, bytes: &[u8]) -> AppResult<()> {
    let dir = target.parent().ok_or_else(|| AppError::invalid("target has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(target).map_err(|e| AppError::from(e.error))?;
    Ok(())
}

/// Make a JPEG thumbnail no wider than `max_w`. Returns the encoded bytes.
pub fn make_thumbnail(bytes: &[u8], max_w: u32) -> AppResult<Vec<u8>> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::new("image", format!("unsupported or corrupt image: {e}")))?;
    let thumb = if img.width() > max_w { img.thumbnail(max_w, u32::MAX) } else { img };
    let rgb = thumb.to_rgb8();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    enc.encode_image(&rgb).map_err(|e| AppError::new("image", e.to_string()))?;
    Ok(out)
}

/// Free space on the volume holding `path`, or None if unknown.
pub fn available_space(path: &Path) -> Option<u64> {
    fs4::available_space(path).ok()
}

pub fn ensure_space(path: &Path, needed: u64) -> AppResult<()> {
    if let Some(avail) = available_space(path) {
        if avail < needed + 200 * 1024 * 1024 {
            return Err(AppError::new(
                "disk_full",
                format!("not enough disk space ({} MB free)", avail / 1024 / 1024),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b/out.bin");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        // no temp files left behind
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn hash_parts_are_separated() {
        assert_ne!(sha256_str(&["ab", "c"]), sha256_str(&["a", "bc"]));
    }

    #[test]
    fn space_is_reported() {
        assert!(available_space(Path::new("/")).unwrap_or(1) > 0);
    }
}
