//! Shared policy for opening untrusted attachments through the OS.
use std::path::{Path, PathBuf};

use crate::Error;
use crate::Result;

/// Resolve symlinks before trusting a DB path. Only regular cache files qualify.
pub fn inside_cache(cache: &Path, path: &Path) -> Option<PathBuf> {
    if std::fs::symlink_metadata(cache)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
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
        "ogg" => &["audio/ogg", "video/ogg"],
        "oga" => &["audio/ogg"],
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

/// In-app rendering is narrower than OS launch, except SVG in image context.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerKind {
    Image,
    Svg,
    Video,
    Pdf,
    Xlsx,
    Docx,
}

pub fn viewer_kind(path: &Path, mime: Option<&str>) -> Option<ViewerKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = mime?.split(';').next()?.trim().to_ascii_lowercase();
    if ext == "svg" && mime == "image/svg+xml" {
        return Some(ViewerKind::Svg);
    }
    if !can_open(path, Some(&mime)) {
        return None;
    }
    Some(match mime.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/avif" | "image/bmp"
        | "image/x-ms-bmp" => ViewerKind::Image,
        "video/mp4" | "video/x-m4v" | "video/quicktime" | "video/webm" | "video/ogg" => {
            ViewerKind::Video
        }
        "application/pdf" => ViewerKind::Pdf,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ViewerKind::Xlsx,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            ViewerKind::Docx
        }
        _ => return None,
    })
}

/// Types permitted over the bounded document IPC, never as navigable documents.
pub fn can_read_document(path: &Path, mime: Option<&str>) -> bool {
    matches!(
        viewer_kind(path, mime),
        Some(ViewerKind::Svg | ViewerKind::Pdf | ViewerKind::Xlsx | ViewerKind::Docx)
    )
}

/// Remove directional formatting that can disguise the real file extension.
pub fn display_name(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')).collect()
}

/// Mark network content before it can be opened or revealed. Fail closed.
#[cfg(target_os = "macos")]
pub fn quarantine(path: &Path) -> Result<()> {
    if xattr::get(path, "com.apple.quarantine")?.is_some() {
        return Ok(());
    }
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

/// A portable basename, using the cache sanitizer before Windows restrictions.
pub fn download_name(name: &str) -> String {
    let clean: String = super::sanitize(name)
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Leave room for collision suffixes within common 255-byte component limits.
    let mut clean: String = clean
        .chars()
        .scan(0, |bytes, c| {
            *bytes += c.len_utf8();
            (*bytes <= 220).then_some(c)
        })
        .collect();
    clean = clean.trim_end_matches(['.', ' ']).to_owned();
    if clean.is_empty() {
        return "file".into();
    }
    let stem = clean
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    }) {
        clean.insert(0, '_');
    }
    clean
}

/// Collision suffix before the last extension; never produces path components.
pub fn download_candidate(name: &str, collision: u64) -> String {
    let name = download_name(name);
    if collision == 0 {
        return name;
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem} ({collision}).{ext}"),
        None => format!("{name} ({collision})"),
    }
}

/// Save directly inside an existing Downloads directory, without replacing anything.
pub fn save_to_downloads(source: &Path, directory: &Path, name: &str) -> Result<PathBuf> {
    let directory = directory
        .canonicalize()
        .map_err(|e| Error::File(format!("downloads_unavailable: {e}")))?;
    if !directory.is_dir() {
        return Err(Error::File("downloads_unavailable: not a directory".into()));
    }
    for collision in 0..=u64::MAX {
        let destination = directory.join(download_candidate(name, collision));
        match save_copy_with(source, &destination, true) {
            Ok(()) => return Ok(destination),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Error::File(format!("downloads_save_failed: {e}"))),
        }
    }
    Err(Error::File(
        "downloads_save_failed: no available filename".into(),
    ))
}

/// Save a downloaded attachment with OS protection, including on an old cache hit.
pub fn save_copy(source: &Path, destination: &Path) -> Result<()> {
    save_copy_with(source, destination, false)
}

fn save_copy_with(source: &Path, destination: &Path, create_new: bool) -> Result<()> {
    if source == destination && !create_new {
        return quarantine(destination);
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create(!create_new)
        .truncate(!create_new)
        .create_new(create_new)
        .open(destination)?;
    // Quarantine before copying bytes; a failed copy never leaves an unprotected file.
    let result = quarantine(destination).and_then(|()| {
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        Ok(())
    });
    if result.is_err() {
        drop(output);
        let _ = std::fs::remove_file(destination);
    }
    result
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
    fn viewer_kind_needs_an_exact_pair_and_is_separate_from_os_launch() {
        use ViewerKind::*;
        for (name, mime, kind) in [
            ("photo.JPG", "IMAGE/JPEG; charset=binary", Image),
            ("a.jpeg", "image/jpeg", Image),
            ("a.png", "image/png", Image),
            ("a.gif", "image/gif", Image),
            ("a.webp", "image/webp", Image),
            ("a.avif", "image/avif", Image),
            ("a.bmp", "image/x-ms-bmp", Image),
            ("a.mp4", "video/mp4", Video),
            ("a.m4v", "video/x-m4v", Video),
            ("a.mov", "video/quicktime", Video),
            ("a.webm", "video/webm", Video),
            ("a.ogv", "video/ogg", Video),
            ("a.ogg", "video/ogg", Video),
            ("a.pdf", "application/pdf", Pdf),
            (
                "a.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                Xlsx,
            ),
            (
                "a.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Docx,
            ),
            ("drawing.SVG", "image/svg+xml; charset=utf-8", Svg),
        ] {
            let got = viewer_kind(Path::new(name), Some(mime));
            assert!(
                matches!(got, Some(k) if std::mem::discriminant(&k) == std::mem::discriminant(&kind)),
                "{name} {mime}: {got:?}"
            );
        }
        for (name, mime) in [
            ("photo", "image/jpeg"),
            ("photo.jpg.command", "image/jpeg"),
            ("photo.png", "image/jpeg"),
            ("photo.jpg", "image/jpg"),
            ("photo.svg", "text/html"),
            ("page.html", "image/svg+xml"),
            ("a.tiff", "image/tiff"),
            ("a.heic", "image/heic"),
            ("a.mkv", "video/x-matroska"),
            ("a.avi", "video/x-msvideo"),
            ("a.oga", "video/ogg"),
            ("a.ogg", "audio/ogg"),
            ("a.txt", "text/plain"),
            ("a.doc", "application/msword"),
        ] {
            assert!(
                viewer_kind(Path::new(name), Some(mime)).is_none(),
                "{name} {mime}"
            );
        }
        assert!(viewer_kind(Path::new("a.png"), None).is_none());
        // SVG is shown in-app as an image but never handed to the OS launcher.
        assert!(!can_open(Path::new("drawing.svg"), Some("image/svg+xml")));
        assert!(can_open(Path::new("a.ogg"), Some("video/ogg")));
        // Only parsed documents and SVG travel over the bounded IPC.
        assert!(can_read_document(
            Path::new("drawing.svg"),
            Some("image/svg+xml")
        ));
        assert!(can_read_document(
            Path::new("a.pdf"),
            Some("application/pdf")
        ));
        assert!(!can_read_document(Path::new("a.png"), Some("image/png")));
        assert!(!can_read_document(Path::new("a.mp4"), Some("video/mp4")));
        assert_eq!(serde_json::to_value(Svg).unwrap(), "svg");
        assert_eq!(serde_json::to_value(Image).unwrap(), "image");
    }
    #[test]
    fn download_names_are_portable_and_never_path_components() {
        for (name, expected) in [
            ("report.pdf", "report.pdf"),
            ("../../etc/passwd", "_.._etc_passwd"),
            ("a\\b:c<d>e\"f|g?h*i.txt", "a_b_c_d_e_f_g_h_i.txt"),
            ("trailing. . ", "trailing"),
            ("con", "_con"),
            ("NUL.txt", "_NUL.txt"),
            ("com1.log", "_com1.log"),
            ("LPT9", "_LPT9"),
            ("console.txt", "console.txt"),
            ("com10.txt", "com10.txt"),
            ("\u{202e}fdp.exe", "fdp.exe"),
            ("...", "file"),
            ("", "file"),
        ] {
            assert_eq!(download_name(name), expected, "{name:?}");
        }
        let long = download_name(&"я".repeat(300));
        assert!(long.len() <= 220 && long.chars().all(|c| c == 'я'));
        assert_eq!(download_candidate("a.tar.gz", 0), "a.tar.gz");
        assert_eq!(download_candidate("a.tar.gz", 2), "a.tar (2).gz");
        assert_eq!(download_candidate("README", 1), "README (1)");
        for n in 0..3 {
            let candidate = download_candidate("../x/y.txt", n);
            assert_eq!(Path::new(&candidate).components().count(), 1);
        }
    }
    #[test]
    fn saving_to_downloads_never_overwrites_and_stays_in_the_folder() {
        let cache = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let source = cache.path().join("cached");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(downloads.path().join("note.txt"), b"mine").unwrap();
        std::fs::create_dir(downloads.path().join("note (1).txt")).unwrap();
        let saved = save_to_downloads(&source, downloads.path(), "note.txt").unwrap();
        let root = downloads.path().canonicalize().unwrap();
        assert_eq!(saved, root.join("note (2).txt"));
        assert_eq!(std::fs::read(&saved).unwrap(), b"new");
        assert_eq!(
            std::fs::read(downloads.path().join("note.txt")).unwrap(),
            b"mine"
        );
        assert!(source.exists());
        let escaped = save_to_downloads(&source, downloads.path(), "../../escape.txt").unwrap();
        assert_eq!(escaped.parent().unwrap(), root);
        #[cfg(target_os = "macos")]
        assert!(xattr::get(&saved, "com.apple.quarantine")
            .unwrap()
            .is_some());
        // A missing folder is an error, not a silent fallback elsewhere.
        let missing = downloads.path().join("missing");
        let error = save_to_downloads(&source, &missing, "note.txt").unwrap_err();
        assert!(error.to_string().contains("downloads_unavailable"));
        let error =
            save_to_downloads(&cache.path().join("absent"), downloads.path(), "x.txt").unwrap_err();
        assert!(error.to_string().contains("downloads_save_failed"));
        assert!(!root.join("x.txt").exists());
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
