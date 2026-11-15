//! The private upload snapshot is owner-only and disappears with the upload.
//! Own test binary: it points TMPDIR at a fresh directory for its whole run.
#![cfg(unix)]
use flek_core::sync::upload::Upload;
use std::os::unix::fs::PermissionsExt;

fn snapshots(dir: &std::path::Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.file_name().to_string_lossy().starts_with("flek-upload-"))
        .collect()
}

#[tokio::test]
async fn snapshots_are_0600_in_the_temp_dir_and_removed_on_drop() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("TMPDIR", tmp.path());
    let source = tmp.path().join("src.bin");
    std::fs::write(&source, b"private bytes").unwrap();

    // CLI path: the file is copied into a private snapshot.
    let from_path = Upload::from_path(&source).await.unwrap();
    // App path: chunks arrive over IPC into the same kind of snapshot.
    let mut staged = Upload::stage("a.bin".into(), 3).unwrap();
    staged.append(0, b"abc").await.unwrap();

    let found = snapshots(tmp.path());
    assert_eq!(found.len(), 2);
    for entry in &found {
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let contents: Vec<_> = found
        .iter()
        .map(|e| std::fs::read(e.path()).unwrap())
        .collect();
    assert!(contents.contains(&b"private bytes".to_vec()) && contents.contains(&b"abc".to_vec()));

    drop(from_path);
    drop(staged);
    assert!(snapshots(tmp.path()).is_empty());
}
