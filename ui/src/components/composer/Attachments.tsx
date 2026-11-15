import { For, Show } from "solid-js";
import { t } from "../../i18n/index";
import { attachmentError, draftAttachments, removeAttachment, retryUpload } from "../../stores/uploads";
import { uploadDisplayName, uploadPercent } from "../../models/upload-model";

export function uploadErrorText(error: string | undefined): string {
  if (error === "upload_empty") return t("Empty files cannot be uploaded.");
  if (error === "upload_too_large") return t("Files must be 100 MiB or smaller.");
  if (error === "upload_bad_name") return t("This filename cannot be uploaded.");
  if (error === "upload_queue_full") return t("Attach up to 10 files at a time.");
  return t("Upload failed. Check your connection and retry.");
}
export default function Attachments(props: { conversationId: string; threadTs?: string }) {
  return <>
    <Show when={draftAttachments(props.conversationId, props.threadTs).length}>
      <div class="compose-attachments" aria-label={t("Attachments")}>
        <For each={draftAttachments(props.conversationId, props.threadTs)}>{entry =>
          <div class="compose-attachment" aria-busy={entry.state === "sending"}>
            <span class="compose-attachment-name">{uploadDisplayName(entry.file.name)}</span>
            <Show when={entry.state === "ready"}><small>{t("Ready to upload")}</small></Show>
            <Show when={entry.state === "sending"}>
              <small>{entry.progress?.stage === "completing" || entry.progress?.stage === "syncing" ? t("Confirming upload…") : t("Uploading file")}</small>
              <progress max="100" value={uploadPercent(entry.progress)} aria-label={t("Uploading file")} />
            </Show>
            <Show when={entry.state === "failed"}>
              <span class="error" role="alert">{uploadErrorText(entry.error)}</span>
              <button type="button" onClick={() => retryUpload(entry.id)}>{t("Retry")}</button>
            </Show>
            <button type="button" onClick={() => removeAttachment(entry.id)}
              disabled={entry.state === "sending" && (entry.progress?.stage === "completing" || entry.progress?.stage === "syncing")}
              aria-label={t("Remove attachment {name}", { name: uploadDisplayName(entry.file.name) })}>×</button>
          </div>
        }</For>
      </div>
    </Show>
    <Show when={attachmentError(props.conversationId, props.threadTs)}>
      <p class="error" role="alert">{uploadErrorText(attachmentError(props.conversationId, props.threadTs))}</p>
    </Show>
  </>;
}
