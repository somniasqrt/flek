//! Remove legacy Slack website data through native APIs, without deleting directories.
use crate::state::AppState;
use tauri::{AppHandle, Manager};

const CLEANUP_VERSION: u32 = 1;

pub async fn upgrade(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _maintenance = state.maintenance.lock().await;
    run_upgrade(state.settings().webview_cleanup_version, clear(app), || {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
        // Read current preferences after cleanup; a concurrent preferences edit
        // must not be overwritten by the pre-cleanup snapshot.
        let mut updated = settings.clone();
        updated.webview_cleanup_version = CLEANUP_VERSION;
        updated
            .save(&flek_core::settings::default_path().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        *settings = updated;
        Ok(())
    })
    .await
}

async fn run_upgrade(
    version: u32,
    clear: impl std::future::Future<Output = Result<(), String>>,
    mark: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if version >= CLEANUP_VERSION {
        return Ok(());
    }
    clear.await?;
    // Persist only after native completion. Any failure retries next boot.
    mark()
}

pub async fn clear(app: &AppHandle) -> Result<(), String> {
    // An ephemeral signin store disappears with its window. No capture may run
    // concurrently: callers hold the maintenance lock, also held by finish().
    if let Some(window) = app.get_webview_window(crate::login::LABEL) {
        window.destroy().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || macos::clear_slack(tx))
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| "website data cleanup timed out; retry sign-in".to_owned())?
            .map_err(|_| "website data cleanup did not complete".to_owned())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Tauri has no portable per-origin removal. On the M7 platforms this
        // fallback also resets app-origin local UI state (not SQLite/settings).
        app.get_webview_window(crate::MAIN)
            .ok_or("main window missing")?
            .clear_all_browsing_data()
            .map_err(|e| e.to_string())
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_slack_record(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "slack.com" || name.ends_with(".slack.com")
}

// Tauri's safe API only offers whole-store deletion. This narrow native bridge
// preserves local app drafts/layout while removing all Slack website data types.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_foundation::NSArray;
    use objc2_web_kit::{WKWebsiteDataRecord, WKWebsiteDataStore};
    use std::{cell::RefCell, ptr::NonNull};

    pub fn clear_slack(done: tokio::sync::oneshot::Sender<()>) {
        let mtm = MainThreadMarker::new().expect("dispatched to main thread");
        // SAFETY: all WebKit operations run on the main thread. The default
        // store is this application's store, never Slack desktop's. Retained
        // objects and copied RcBlocks keep captures alive until completion.
        unsafe {
            let store = WKWebsiteDataStore::defaultDataStore(mtm);
            let types = WKWebsiteDataStore::allWebsiteDataTypes(mtm);
            let done = RefCell::new(Some(done));
            let complete = RcBlock::new(move || {
                if let Some(done) = done.borrow_mut().take() {
                    let _ = done.send(());
                }
            });
            let remove_store = store.clone();
            let remove_types = types.clone();
            let records = RcBlock::new(move |records: NonNull<NSArray<WKWebsiteDataRecord>>| {
                // WebKit supplies a non-null array valid throughout the callback;
                // to_vec retains each record before that callback returns.
                let selected: Vec<_> = records
                    .as_ref()
                    .to_vec()
                    .into_iter()
                    .filter(|record| super::is_slack_record(&record.displayName().to_string()))
                    .collect();
                let selected = NSArray::from_retained_slice(&selected);
                remove_store.removeDataOfTypes_forDataRecords_completionHandler(
                    &remove_types,
                    &selected,
                    &complete,
                );
            });
            store.fetchDataRecordsOfTypes_completionHandler(&types, &records);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn upgrade_marks_only_after_success_and_retries_failures() {
        use std::cell::RefCell;
        let events = RefCell::new(Vec::new());
        run_upgrade(
            0,
            async {
                events.borrow_mut().push("clear");
                Ok(())
            },
            || {
                events.borrow_mut().push("mark");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(*events.borrow(), ["clear", "mark"]);
        events.borrow_mut().clear();
        assert!(run_upgrade(0, async { Err("failed".into()) }, || {
            events.borrow_mut().push("mark");
            Ok(())
        })
        .await
        .is_err());
        assert!(events.borrow().is_empty());
        run_upgrade(
            CLEANUP_VERSION,
            async { panic!("already upgraded") },
            || panic!("must not rewrite marker"),
        )
        .await
        .unwrap();
        assert!(
            run_upgrade(0, async { Ok(()) }, || Err("save failed".into()))
                .await
                .is_err()
        );
    }

    #[test]
    fn cleanup_scope_only_matches_slack_website_records() {
        for name in [
            "slack.com",
            "app.slack.com",
            "SLACK.COM",
            "workspace.slack.com",
        ] {
            assert!(is_slack_record(name));
        }
        for name in [
            "tauri.localhost",
            "localhost",
            "app.flek.desktop",
            "slack.com.evil.test",
            "notslack.com",
            "",
        ] {
            assert!(!is_slack_record(name));
        }
    }
}
