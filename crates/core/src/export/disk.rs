//! Portable, symlink-refusing, atomic writes and protected cached copies.
use super::read::File;
use crate::{files, store::unfurls::url_hash, Error, Result};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(super) fn vault(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    // Refuse symlinks below the supplied vault. Its existing ancestors may be
    // platform aliases (e.g. /tmp on macOS), so canonicalize the root once.
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::File("export vault cannot be a symlink".into()));
    }
    fs::create_dir_all(&path)?;
    Ok(path.canonicalize()?)
}

pub(super) fn directory(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::File(
            "export directory is not a regular directory".into(),
        ));
    }
    Ok(path)
}

fn check_target(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            Err(Error::File("export target is not a regular file".into()))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn write_changed(path: &Path, content: &[u8]) -> Result<bool> {
    check_target(path)?;
    match fs::read(path) {
        Ok(old) if old == content => return Ok(false),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(content)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(true)
}

pub(super) enum CopyState {
    Copied,
    Unchanged,
    Unavailable,
}
pub(super) struct Copy {
    pub source: Option<PathBuf>,
    pub relative: Option<String>,
    pub state: CopyState,
}

fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha1::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn attachment(root: &Path, cache: &Path, file: &File) -> Result<Copy> {
    let unavailable = || Copy {
        source: None,
        relative: None,
        state: CopyState::Unavailable,
    };
    let Some(source) = file
        .local_path
        .as_deref()
        .and_then(|p| files::inside_cache(cache, Path::new(p)))
    else {
        return Ok(unavailable());
    };
    // Hash the content as well as the id: changed attachments never overwrite a
    // previous copy; re-exports also detect same-size changes and cache eviction.
    let content_hash = digest(&source)?;
    let folder_name = format!("{}-{content_hash}", url_hash(&file.id));
    let attachments = directory(root, "attachments")?;
    let folder = directory(&attachments, &folder_name)?;
    let name = files::download_name(
        &file
            .name
            .as_deref()
            .unwrap_or(&file.id)
            .replace(['#', '^', '[', ']', '|'], "_"),
    );
    let target = folder.join(&name);
    check_target(&target)?;
    let state = if target.is_file() && digest(&target)? == content_hash {
        CopyState::Unchanged
    } else {
        let temporary = tempfile::NamedTempFile::new_in(&folder)?;
        files::save_copy(&source, temporary.path())?;
        // The cache can be replaced by sync during export. Do not publish a
        // copy whose bytes disagree with its content-addressed destination.
        if digest(temporary.path())? != content_hash {
            return Err(Error::File(
                "cached attachment changed during export; retry".into(),
            ));
        }
        temporary.persist(&target).map_err(|e| e.error)?;
        CopyState::Copied
    };
    Ok(Copy {
        source: Some(source),
        relative: Some(format!("attachments/{folder_name}/{name}")),
        state,
    })
}

pub(super) fn vault_relative(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}
