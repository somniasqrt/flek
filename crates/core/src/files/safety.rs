//! Shared policy for opening untrusted attachments through the OS.
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use crate::Error;
use crate::Result;

/// Resolve symlinks before trusting a DB path. Only regular cache files qualify.
pub fn inside_cache(cache: &Path, path: &Path) -> Option<PathBuf> {
    let cache = cache.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    (path.starts_with(cache) && path.is_file()).then_some(path)
}

/// An exact extension/MIME pair, never a sender's title or filetype label.
/// This is a launch policy, not a guarantee that document contents are harmless.
pub fn can_open(path: &Path, mime: Option<&str>) -> bool {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return false;
    };
    let mime = mime
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let allowed: &[&str] = match ext.to_ascii_lowercase().as_str() {
        "png" => &["image/png"],
        "jpg" | "jpeg" => &["image/jpeg"],
        "gif" => &["image/gif"],
        "webp" => &["image/webp"],
        "bmp" => &["image/bmp", "image/x-ms-bmp"],
        "tif" | "tiff" => &["image/tiff"],
        "heic" => &["image/heic"],
        "heif" => &["image/heif"],
        "avif" => &["image/avif"],
        "pdf" => &["application/pdf"],
        "txt" => &["text/plain"],
        "md" | "markdown" => &["text/markdown", "text/plain", "text/x-markdown"],
        "csv" => &["text/csv"],
        "doc" => &["application/msword"],
        "docx" => &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
        "xls" => &["application/vnd.ms-excel"],
        "xlsx" => &["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"],
        "ppt" => &["application/vnd.ms-powerpoint"],
        "pptx" => &["application/vnd.openxmlformats-officedocument.presentationml.presentation"],
        "odt" => &["application/vnd.oasis.opendocument.text"],
        "ods" => &["application/vnd.oasis.opendocument.spreadsheet"],
        "odp" => &["application/vnd.oasis.opendocument.presentation"],
        "rtf" => &["application/rtf", "text/rtf"],
        "pages" => &[
            "application/vnd.apple.pages",
            "application/x-iwork-pages-sffpages",
        ],
        "numbers" => &[
            "application/vnd.apple.numbers",
            "application/x-iwork-numbers-sffnumbers",
        ],
        "key" => &[
            "application/vnd.apple.keynote",
            "application/x-iwork-keynote-sffkey",
        ],
        "mp3" => &["audio/mpeg"],
        "m4a" => &["audio/mp4", "audio/x-m4a"],
        "aac" => &["audio/aac"],
        "wav" => &["audio/wav", "audio/x-wav", "audio/vnd.wave"],
        "flac" => &["audio/flac", "audio/x-flac"],
        "ogg" | "oga" => &["audio/ogg"],
        "opus" => &["audio/opus", "audio/ogg"],
        "mp4" | "m4v" => &["video/mp4", "video/x-m4v"],
        "mov" => &["video/quicktime"],
        "webm" => &["video/webm"],
        "ogv" => &["video/ogg"],
        "mkv" => &["video/x-matroska"],
        "avi" => &["video/x-msvideo"],
        "mpeg" | "mpg" => &["video/mpeg"],
        _ => &[],
    };
    allowed.contains(&mime.as_str())
}

/// Remove directional formatting that can disguise the real file extension.
pub fn display_name(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')).collect()
}

/// Mark network content before it can be opened or revealed. Fail closed.
#[cfg(target_os = "macos")]
pub fn quarantine(path: &Path) -> Result<()> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::File("cannot determine quarantine time".into()))?
        .as_secs();
    let value = format!("0081;{time:x};Flek;{}", uuid::Uuid::new_v4());
    xattr::set(path, "com.apple.quarantine", value.as_bytes())?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn quarantine(_path: &Path) -> Result<()> {
    Ok(())
}

/// Save a downloaded attachment with OS protection, including on an old cache hit.
pub fn save_copy(source: &Path, destination: &Path) -> Result<()> {
    if source != destination {
        std::fs::copy(source, destination)?;
    }
    if let Err(error) = quarantine(destination) {
        if source != destination {
            let _ = std::fs::remove_file(destination);
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_extension_and_mime_must_agree() {
        for (name, mime) in [
            ("a.PDF", "application/pdf"),
            ("a.png", "image/png"),
            ("a.txt", "text/plain; charset=utf-8"),
            (
                "a.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            ("a.mp3", "audio/mpeg"),
        ] {
            assert!(can_open(Path::new(name), Some(mime)), "{name}");
        }
        for name in [
            "x.pdf.command",
            "x.terminal",
            "x.inetloc",
            "x.webloc",
            "x.app",
            "x.zip",
            "x.dmg",
            "x.pkg",
            "x",
            "x.svg",
            "x.html",
            "x.docm",
        ] {
            assert!(
                !can_open(Path::new(name), Some("application/pdf")),
                "{name}"
            );
        }
        assert!(!can_open(Path::new("x.pdf"), Some("image/png")));
        assert!(!can_open(Path::new("x.png"), Some("image/jpeg")));
        assert!(!can_open(Path::new("x.pdf"), None));
    }
    #[test]
    fn strips_all_bidi_controls() {
        assert_eq!(display_name("a\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}.command"), "a.command");
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn save_copy_adds_quarantine_even_when_source_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.command");
        let dest = dir.path().join("copy.command");
        std::fs::write(&source, "synthetic").unwrap();
        save_copy(&source, &dest).unwrap();
        assert!(xattr::get(&dest, "com.apple.quarantine").unwrap().is_some());
    }
}
