//! Upload IPC owns handles and events; core owns staging, limits and transport.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use flek_core::sync::upload::{Destination, Upload, UploadProgress, Uploaded};
use tauri::{ipc::Channel, State};
use tokio::sync::watch;

use super::{text, CmdResult};
use crate::state::AppState;

pub struct Slot {
    team: String,
    upload: tokio::sync::Mutex<Upload>,
    cancel: watch::Sender<bool>,
    receipt: Mutex<Option<Uploaded>>,
}
#[derive(Default)]
pub struct Uploads(Mutex<HashMap<String, Arc<Slot>>>);
impl Uploads {
    fn get(&self, id: &str, team: &str) -> CmdResult<Arc<Slot>> {
        self.0
            .lock()
            .map_err(text)?
            .get(id)
            .filter(|s| s.team == team)
            .cloned()
            .ok_or_else(|| "upload_unavailable".into())
    }
    /// Drop private snapshots and cancel work before credentials are removed.
    pub async fn clear(&self) {
        let slots: Vec<_> = self
            .0
            .lock()
            .expect("uploads lock")
            .drain()
            .map(|(_, s)| s)
            .collect();
        for slot in &slots {
            slot.cancel.send_replace(true);
        }
        for slot in slots {
            drop(slot.upload.lock().await);
        }
    }
}

#[tauri::command]
pub async fn prepare_upload(
    state: State<'_, AppState>,
    name: String,
    size: u64,
) -> CmdResult<String> {
    let _maintenance = state.maintenance.lock().await;
    let team = state
        .team_id()
        .filter(|_| state.client().is_some())
        .ok_or("not signed in to Slack")?;
    let upload = Upload::stage(name, size).map_err(text)?;
    let id = flek_core::api::chat::new_client_msg_id();
    let mut uploads = state.uploads.0.lock().map_err(text)?;
    // Bounded disk reservation even if a frontend abandons staged files.
    if uploads.len() >= 10 {
        return Err("upload_queue_full".into());
    }
    uploads.insert(
        id.clone(),
        Arc::new(Slot {
            team,
            upload: tokio::sync::Mutex::new(upload),
            cancel: watch::channel(false).0,
            receipt: Mutex::new(None),
        }),
    );
    Ok(id)
}

#[tauri::command]
pub async fn append_upload(
    state: State<'_, AppState>,
    id: String,
    offset: u64,
    bytes: Vec<u8>,
) -> CmdResult<()> {
    let team = state.team_id().ok_or("not signed in to Slack")?;
    let slot = state.uploads.get(&id, &team)?;
    if *slot.cancel.borrow() {
        return Err("upload_cancelled".into());
    }
    let mut upload = slot.upload.try_lock().map_err(|_| "upload_busy")?;
    upload.append(offset, &bytes).await.map_err(text)
}

#[tauri::command]
pub async fn send_upload(
    state: State<'_, AppState>,
    id: String,
    destination: Destination,
    on_progress: Channel<UploadProgress>,
) -> CmdResult<Uploaded> {
    let team = state.team_id().ok_or("not signed in to Slack")?;
    let client = state.client().ok_or("not signed in to Slack")?;
    let slot = state.uploads.get(&id, &team)?;
    let mut upload = slot.upload.try_lock().map_err(|_| "upload_busy")?;
    if let Some(receipt) = slot.receipt.lock().map_err(text)?.clone() {
        return Ok(receipt);
    }
    let uploaded = upload
        .send(
            &client,
            &state.store,
            destination,
            slot.cancel.subscribe(),
            Arc::new(move |p| {
                let _ = on_progress.send(p);
            }),
        )
        .await
        .map_err(text)?;
    if let Some(change) = uploaded.change.clone() {
        let _ = state.bus.send(change);
    }
    // Keep a receipt until frontend acknowledgement, so a lost IPC result is replayable.
    *slot.receipt.lock().map_err(text)? = Some(uploaded.clone());
    Ok(uploaded)
}

#[tauri::command]
pub async fn discard_upload(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let team = state.team_id().ok_or("not signed in to Slack")?;
    let slot = state.uploads.get(&id, &team)?;
    slot.cancel.send_replace(true);
    // Completion, if already started, must settle before deleting its snapshot.
    let _upload = slot.upload.lock().await;
    state.uploads.0.lock().map_err(text)?.remove(&id);
    Ok(())
}
