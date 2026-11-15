//! Documented external uploads. The ticket URL never receives session credentials.
use std::{path::Path, sync::Arc, time::Duration};

use futures_util::stream;
use serde::Deserialize;
use tokio::io::AsyncReadExt;

use super::{
    client::{retry_after, MAX_RETRIES},
    Client,
};
use crate::{Error, Result, Secret};

#[derive(Debug)]
pub struct UploadTicket {
    pub file_id: String,
    url: Secret,
}

/// Sharing options for completion; blocks and initial_comment are mutually exclusive.
pub struct UploadShare<'a> {
    pub filename: &'a str,
    pub channel: &'a str,
    pub thread: Option<&'a str>,
    pub comment: &'a str,
    pub blocks: Option<&'a [serde_json::Value]>,
}

impl Client {
    pub async fn get_upload_url(&self, filename: &str, length: u64) -> Result<UploadTicket> {
        #[derive(Deserialize)]
        struct Raw {
            file_id: String,
            upload_url: String,
        }
        let raw: Raw = self
            .call(
                "files.getUploadURLExternal",
                &[("filename", filename), ("length", &length.to_string())],
            )
            .await?;
        self.validate_upload_url(&raw.upload_url)?;
        if raw.file_id.is_empty() {
            return Err(Error::File("missing upload file id".into()));
        }
        Ok(UploadTicket {
            file_id: raw.file_id,
            url: Secret::new(raw.upload_url),
        })
    }

    /// Stream a private snapshot with bounded memory. Progress counts bytes handed
    /// to HTTP, not server acknowledgements. Each 429 restarts from byte zero.
    pub async fn upload_bytes(
        &self,
        ticket: &UploadTicket,
        path: &Path,
        length: u64,
        progress: Arc<dyn Fn(u64) + Send + Sync>,
    ) -> Result<()> {
        let url = self.validate_upload_url(ticket.url.expose())?;
        // No default session headers, cookie jar, auth, redirects or proxy.
        // A redirect must never forward the user's file to another destination.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(600))
            .build()?;
        for attempt in 0..=MAX_RETRIES {
            let file = tokio::fs::File::open(path).await?;
            if file.metadata().await?.len() != length {
                return Err(Error::File("upload snapshot size changed".into()));
            }
            progress(0);
            let callback = progress.clone();
            let body = stream::try_unfold((file, 0u64), move |(mut file, sent)| {
                let callback = callback.clone();
                async move {
                    if sent == length {
                        return Ok::<_, std::io::Error>(None);
                    }
                    let mut chunk = vec![0; (length - sent).min(64 * 1024) as usize];
                    let n = file.read(&mut chunk).await?;
                    if n == 0 {
                        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
                    }
                    chunk.truncate(n);
                    callback(sent + n as u64);
                    Ok(Some((chunk, (file, sent + n as u64))))
                }
            });
            let response = http
                .post(url.clone())
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .header(reqwest::header::CONTENT_LENGTH, length)
                .body(reqwest::Body::wrap_stream(body))
                .send()
                .await
                .map_err(|e| Error::Http(e.without_url()))?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES
            {
                tokio::time::sleep(retry_after(response.headers())).await;
                continue;
            }
            if !response.status().is_success() {
                return Err(Error::HttpStatus(response.status().as_u16()));
            }
            return Ok(());
        }
        unreachable!("bounded retry loop always returns")
    }

    pub async fn complete_upload(
        &self,
        ticket: &UploadTicket,
        share: UploadShare<'_>,
    ) -> Result<()> {
        let files =
            serde_json::json!([{ "id": ticket.file_id, "title": share.filename }]).to_string();
        let comment = super::chat::escape_text(share.comment);
        let blocks = share
            .blocks
            .filter(|b| !b.is_empty())
            .map(serde_json::to_string)
            .transpose()?;
        let mut params = vec![("files", files.as_str()), ("channel_id", share.channel)];
        if let Some(thread) = share.thread {
            params.push(("thread_ts", thread));
        }
        if let Some(blocks) = blocks.as_deref() {
            params.push(("blocks", blocks));
        } else if !comment.is_empty() {
            params.push(("initial_comment", &comment));
        }
        let _: serde_json::Value = self.call("files.completeUploadExternal", &params).await?;
        Ok(())
    }
}
