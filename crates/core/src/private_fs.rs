//! Modes on app-owned paths only. Never change a shared OS parent or follow symlinks.
use crate::Result;
use std::path::Path;

pub(crate) fn mode(path: &Path, _mode: u32) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(crate::Error::File(
            "refusing symlink in private storage".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(_mode))?;
    }
    Ok(())
}

pub(crate) fn dir(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    mode(path, 0o700)
}

pub(crate) fn tree(path: &Path) -> Result<()> {
    dir(path)?;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        if entry.file_type()?.is_dir() {
            tree(&entry.path())?;
        } else if entry.file_type()?.is_file() {
            mode(&entry.path(), 0o600)?;
        }
    }
    Ok(())
}

pub(crate) fn database(path: &Path) -> Result<()> {
    mode(path, 0o600)?;
    for suffix in ["-wal", "-shm"] {
        let mut side = path.as_os_str().to_owned();
        side.push(suffix);
        match mode(Path::new(&side), 0o600) {
            Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            result => result?,
        }
    }
    Ok(())
}
