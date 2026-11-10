//! Offline storage inspection and cache maintenance.
use std::path::Path;

use serde::Serialize;

use crate::{store::Store, Result};

#[derive(Debug, Serialize)]
pub struct StorageInfo {
    pub db_path: String,
    pub db_bytes: u64,
    pub cache_bytes: u64,
}

/// Does not follow symlinks outside the cache.
pub fn directory_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            total += directory_size(&entry.path())?;
        } else if kind.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

pub fn info(store: &Store, cache: &Path) -> Result<StorageInfo> {
    let path = store.path();
    let mut db_bytes = 0;
    if let Some(path) = path {
        for suffix in ["", "-wal", "-shm"] {
            let file = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
            match std::fs::metadata(file) {
                Ok(m) => db_bytes += m.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(StorageInfo {
        db_path: path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        db_bytes,
        cache_bytes: directory_size(cache)?,
    })
}

/// Call with background downloads stopped. Keep public avatars/emoji/link images
/// in `_links`; invalidate every downloaded file and its rendered preview.
pub async fn clear(store: &Store, cache: &Path) -> Result<Vec<String>> {
    if cache.exists() {
        for entry in std::fs::read_dir(cache)? {
            let entry = entry?;
            if entry.file_name() == "_links" {
                continue;
            }
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    store
        .call(|c| {
            let conversations = {
                let mut stmt = c.prepare(
                    "SELECT DISTINCT conversation_id
                     FROM message_files",
                )?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            c.execute(
                "UPDATE files
                 SET local_path = NULL, preview_path = NULL",
                [],
            )?;
            Ok(conversations)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn size_and_clear_preserve_archive_and_public_images() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("files");
        std::fs::create_dir_all(cache.join("F1")).unwrap();
        std::fs::create_dir_all(cache.join("_links")).unwrap();
        std::fs::write(cache.join("F1/file"), b"abc").unwrap();
        std::fs::write(cache.join("_links/avatar"), b"12").unwrap();
        let store = Store::open(&dir.path().join("archive.db")).unwrap();
        store
            .with_conn(|c| {
                c.execute(
                    "INSERT INTO files (id, local_path, preview_path, raw_json)
                 VALUES ('F1', 'file', 'preview', '{}')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(info(&store, &cache).unwrap().cache_bytes, 5);
        clear(&store, &cache).await.unwrap();
        assert_eq!(directory_size(&cache).unwrap(), 2);
        assert!(!cache.join("F1").exists());
        store
            .with_conn(|c| {
                let paths: (Option<String>, Option<String>) = c.query_row(
                    "SELECT local_path, preview_path FROM files WHERE id = 'F1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                assert_eq!(paths, (None, None));
                Ok(())
            })
            .unwrap();
    }
}
