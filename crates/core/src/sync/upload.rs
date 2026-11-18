//! Resumable uploads with private snapshots and DB-only authoritative output.
use std::{path::Path, sync::Arc};

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

use super::live::Change;
use crate::{
    api::{upload::UploadTicket, Client},
    store::Store,
    Error, Result,
};

/// Flek's bounded staging policy, independent of automatic download preferences.
pub const MAX_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_UPLOAD_CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct UploadProgress {
    pub stage: &'static str,
    pub sent: u64,
    pub total: u64,
    pub file_id: Option<String>,
}
pub type Progress = Arc<dyn Fn(UploadProgress) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub conversation_id: String,
    pub thread_ts: Option<String>,
    pub comment: String,
    #[serde(default)]
    pub blocks: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Uploaded {
    pub file_id: String,
    pub conversation_id: String,
    pub ts: Option<String>,
    pub not_stored_locally: bool,
    /// A lost finalization response must not be retried as a new upload.
    pub completion_unknown: bool,
    #[serde(skip)]
    pub change: Option<Change>,
}

/// The snapshot and ticket live only for this process. Retries keep the ticket,
/// uploaded bytes and frozen destination; completion is never blindly replayed.
#[derive(Debug)]
pub struct Upload {
    snapshot: tempfile::NamedTempFile,
    filename: String,
    length: u64,
    staged: u64,
    destination: Option<Destination>,
    ticket: Option<UploadTicket>,
    bytes_uploaded: bool,
    completion_started: bool,
    completion_unknown: bool,
}

pub fn validate_file(filename: &str, length: u64) -> Result<()> {
    if filename.is_empty()
        || filename.len() > 255
        || filename
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(Error::Usage("invalid upload filename".into()));
    }
    if length == 0 || length > MAX_UPLOAD_BYTES {
        return Err(Error::Usage(
            "upload size must be between 1 byte and 100 MiB".into(),
        ));
    }
    Ok(())
}

impl Upload {
    /// A 0600 temporary snapshot. No caller-supplied paths are accepted by IPC.
    pub fn stage(filename: String, length: u64) -> Result<Self> {
        validate_file(&filename, length)?;
        Ok(Self {
            snapshot: tempfile::Builder::new().prefix("flek-upload-").tempfile()?,
            filename,
            length,
            staged: 0,
            destination: None,
            ticket: None,
            bytes_uploaded: false,
            completion_started: false,
            completion_unknown: false,
        })
    }

    pub async fn from_path(path: &Path) -> Result<Self> {
        let not_a_file = || Error::Usage("upload requires a regular file".into());
        let mut source = match tokio::fs::File::open(path).await {
            Ok(source) => source,
            // Windows cannot open a directory as a file.
            Err(_) if tokio::fs::metadata(path).await.is_ok_and(|m| !m.is_file()) => {
                return Err(not_a_file())
            }
            Err(e) => return Err(e.into()),
        };
        let metadata = source.metadata().await?;
        if !metadata.is_file() {
            return Err(not_a_file());
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::Usage("upload filename must be UTF-8".into()))?;
        let mut upload = Self::stage(name.to_owned(), metadata.len())?;
        let mut buffer = vec![0; MAX_UPLOAD_CHUNK];
        loop {
            let n = source.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            upload.append(upload.staged, &buffer[..n]).await?;
        }
        if upload.staged != upload.length {
            return Err(Error::File("source changed while staging upload".into()));
        }
        Ok(upload)
    }

    pub async fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if self.destination.is_none()
            && !bytes.is_empty()
            && bytes.len() <= MAX_UPLOAD_CHUNK
            && offset
                .checked_add(bytes.len() as u64)
                .is_some_and(|end| end <= self.staged)
        {
            use tokio::io::AsyncSeekExt;
            let mut file = tokio::fs::File::open(self.snapshot.path()).await?;
            file.seek(std::io::SeekFrom::Start(offset)).await?;
            let mut previous = vec![0; bytes.len()];
            file.read_exact(&mut previous).await?;
            if previous == bytes {
                return Ok(());
            }
            return Err(Error::Usage("upload chunk does not match snapshot".into()));
        }
        if self.destination.is_some()
            || offset != self.staged
            || bytes.is_empty()
            || bytes.len() > MAX_UPLOAD_CHUNK
            || bytes.len() as u64 > self.length - self.staged
        {
            return Err(Error::Usage("invalid upload chunk or offset".into()));
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(self.snapshot.path())
            .await?;
        // A failed/partially written append is replayable at the same offset.
        use tokio::io::AsyncSeekExt;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        self.staged += bytes.len() as u64;
        Ok(())
    }

    /// Cancellation interrupts ticket/rate-limit/byte work. Once completion
    /// starts it is allowed to settle, because the file may already be shared.
    pub async fn send(
        &mut self,
        client: &Client,
        store: &Store,
        destination: Destination,
        mut cancel: watch::Receiver<bool>,
        progress: Progress,
    ) -> Result<Uploaded> {
        if self.staged != self.length {
            return Err(Error::Usage("upload is not fully staged".into()));
        }
        if destination.conversation_id.trim().is_empty() {
            return Err(Error::Usage("upload needs a channel".into()));
        }
        if destination
            .thread_ts
            .as_deref()
            .is_some_and(|t| !crate::references::valid_ts(t))
        {
            return Err(Error::Usage("invalid thread timestamp".into()));
        }
        match &self.destination {
            Some(previous) if previous != &destination => {
                return Err(Error::Usage("upload destination is already fixed".into()))
            }
            None => self.destination = Some(destination.clone()),
            _ => {}
        }
        let report = |stage| {
            progress(UploadProgress {
                stage,
                sent: 0,
                total: self.length,
                file_id: None,
            })
        };
        if !self.completion_started {
            if *cancel.borrow() {
                return Err(Error::Usage("upload cancelled".into()));
            }
            let work = async {
                if self.ticket.is_none() {
                    report("preparing");
                    self.ticket = Some(client.get_upload_url(&self.filename, self.length).await?);
                }
                if !self.bytes_uploaded {
                    let total = self.length;
                    let callback = progress.clone();
                    let file_id = self.ticket.as_ref().expect("ticket").file_id.clone();
                    client
                        .upload_bytes(
                            self.ticket.as_ref().expect("ticket"),
                            self.snapshot.path(),
                            total,
                            Arc::new(move |sent| {
                                callback(UploadProgress {
                                    stage: "uploading",
                                    sent,
                                    total,
                                    file_id: Some(file_id.clone()),
                                })
                            }),
                        )
                        .await?;
                    self.bytes_uploaded = true;
                }
                Ok::<_, Error>(())
            };
            tokio::select! {
                biased;
                _ = cancelled(&mut cancel) => return Err(Error::Usage("upload cancelled".into())),
                result = work => result?,
            }
            if *cancel.borrow() {
                return Err(Error::Usage("upload cancelled".into()));
            }
            report("completing");
            self.completion_started = true;
            if let Err(error) = client
                .complete_upload(
                    self.ticket.as_ref().expect("ticket"),
                    crate::api::upload::UploadShare {
                        filename: &self.filename,
                        channel: &destination.conversation_id,
                        thread: destination.thread_ts.as_deref(),
                        comment: &destination.comment,
                        blocks: destination.blocks.as_deref(),
                    },
                )
                .await
            {
                // Definitive refusals can safely retry completion on this same ticket.
                // Transport/parse/server errors may follow a successful remote write.
                let ambiguous = match &error {
                    Error::Slack { code, .. } => matches!(
                        code.as_str(),
                        "internal_error"
                            | "fatal_error"
                            | "request_timeout"
                            | "service_unavailable"
                    ),
                    Error::HttpStatus(status) => *status >= 500 || *status == 408,
                    Error::Http(_) | Error::Json(_) => true,
                    _ => false,
                };
                if !ambiguous {
                    self.completion_started = false;
                    return Err(error);
                }
                self.completion_unknown = true;
            }
        }
        report("syncing");
        // Do not synthesize a file_share timestamp from the local clock or file's
        // created field. Only real history/live messages establish that identity.
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            super::fresh_conversation(
                client,
                store,
                &destination.conversation_id,
                destination.thread_ts.as_deref(),
            ),
        )
        .await;
        let file_id = self.ticket.as_ref().expect("ticket").file_id.clone();
        let id = file_id.clone();
        let channel = destination.conversation_id.clone();
        let thread = destination.thread_ts.clone();
        let ts = store
            .read(move |c| {
                Ok(c.query_row(
                    "
                    SELECT mf.ts
                    FROM message_files mf
                    JOIN messages m ON m.conversation_id = mf.conversation_id AND m.ts = mf.ts
                    WHERE mf.file_id = ?1
                    AND mf.conversation_id = ?2
                    AND ((?3 IS NULL AND (m.thread_ts IS NULL OR m.thread_ts = m.ts))
                        OR m.thread_ts = ?3)
                    AND m.deleted = 0
                    ORDER BY mf.ts DESC
                    LIMIT 1
                ",
                    rusqlite::params![id, channel, thread],
                    |r| r.get::<_, String>(0),
                )
                .optional()?)
            })
            .await
            .ok()
            .flatten();
        let change = Some(Change::History {
            conversation_id: destination.conversation_id.clone(),
        });
        Ok(Uploaded {
            file_id,
            conversation_id: destination.conversation_id,
            not_stored_locally: ts.is_none(),
            completion_unknown: self.completion_unknown && ts.is_none(),
            ts,
            change,
        })
    }
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
