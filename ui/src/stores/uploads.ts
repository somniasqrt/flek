// In-memory attachment drafts survive channel/thread navigation. Files and
// tickets are never persisted to localStorage; accepted copies come from DB.
import type { RichBlock } from "../models/rich-compose";
import { createStore, produce } from "solid-js/store";
import { api, type Message } from "../api";
import { escapeText, newClientMsgId, nowTs } from "../models/compose";
import { MAX_UPLOAD_FILES, UPLOAD_CHUNK_BYTES, uploadFileError, uploadPercent, type UploadProgress } from "../models/upload-model";

export type Attachment = {
  id: string; conversationId: string; threadTs: string | null;
  file: File; handle?: string; offset: number;
  state: "ready" | "sending" | "failed" | "accepted";
  progress?: UploadProgress; error?: string; comment?: string; blocks?: RichBlock[];
  ts: string; fileId?: string; sentTs?: string | null; uncertain?: boolean;
};
const [attachments, setAttachments] = createStore<Attachment[]>([]);
const update = (id: string, f: (e: Attachment) => void) => setAttachments(e => e.id === id, produce(f));
const exists = (id: string) => attachments.some(e => e.id === id);
export const draftAttachments = (conversationId: string, threadTs?: string | null) => attachments.filter(e => e.conversationId === conversationId && e.threadTs === (threadTs ?? null) && e.state !== "accepted");

export function addAttachments(conversationId: string, threadTs: string | null, files: File[]): string | null {
  let error: string | null = null;
  for (const file of files) {
    const invalid = uploadFileError(file);
    if (invalid) { error = invalid; continue; }
    if (attachments.filter(e => e.state !== "accepted").length >= MAX_UPLOAD_FILES) { error = "upload_queue_full"; break; }
    setAttachments(list => [...list, { id: newClientMsgId(), conversationId, threadTs, file, offset: 0, state: "ready", ts: nowTs(Date.now()) }]);
  }
  return error;
}

export function sendAttachments(conversationId: string, threadTs: string | null, comment: string, blocks?: RichBlock[]) {
  const ready = draftAttachments(conversationId, threadTs).filter(e => e.state === "ready");
  ready.forEach((entry, index) => {
    update(entry.id, e => { e.comment = index === 0 ? comment : ""; e.blocks = index === 0 && comment ? blocks : undefined; e.ts = nowTs(Date.now()); e.state = "sending"; });
  });
  // Sequential sharing keeps the initial comment on the first file and avoids bursts.
  void (async () => { for (const entry of ready) if (exists(entry.id)) await deliver(entry.id); })();
}
async function deliver(id: string) {
  const entry = attachments.find(e => e.id === id);
  if (!entry) return;
  update(id, e => { e.state = "sending"; e.error = undefined; });
  try {
    let handle = entry.handle;
    if (!handle) {
      handle = await api.prepareUpload(entry.file.name, entry.file.size);
      if (!exists(id)) { await api.discardUpload(handle); return; }
      update(id, e => { e.handle = handle; });
    }
    while (entry.offset < entry.file.size) {
      if (!exists(id)) return;
      const offset = entry.offset;
      const bytes = new Uint8Array(await entry.file.slice(offset, offset + UPLOAD_CHUNK_BYTES).arrayBuffer());
      if (!exists(id)) return;
      await api.appendUpload(handle, offset, bytes);
      if (!exists(id)) return;
      update(id, e => { e.offset = offset + bytes.length; e.progress = { stage: "staging", sent: e.offset, total: e.file.size }; });
    }
    const sent = await api.sendUpload(handle, entry.conversationId, entry.threadTs, entry.comment ?? "", p => update(id, e => { e.progress = p; e.fileId = p.file_id ?? e.fileId; }), entry.blocks);
    if (!exists(id)) return;
    update(id, e => {
      e.state = "accepted"; e.fileId = sent.file_id; e.sentTs = sent.ts; e.uncertain = sent.completion_unknown;
      e.handle = undefined;
    });
    void api.discardUpload(handle).catch(() => {});
    // Release the browser File after acceptance; the remaining receipt is tiny.
    update(id, e => { e.file = new File([], e.file.name); });
  } catch (error) {
    if (exists(id)) update(id, e => { e.state = "failed"; e.error = String(error); });
  }
}
export function retryUpload(id: string): boolean {
  const entry = attachments.find(e => e.id === id);
  if (!entry) return false;
  if (entry.state === "failed") void deliver(id);
  return true;
}
export function removeAttachment(id: string): boolean {
  const entry = attachments.find(e => e.id === id);
  if (!entry) return false;
  setAttachments(list => list.filter(e => e.id !== id));
  if (entry.handle) void api.discardUpload(entry.handle).catch(() => {});
  return true;
}
export function clearUploads() {
  for (const entry of [...attachments]) removeAttachment(entry.id);
  for (const key of Object.keys(errors)) setErrors(key, undefined);
}

export function pendingUploadsFor(conversationId: string, threadTs: string | null, stored: readonly Message[], me: string | null): Message[] {
  return attachments.filter(e => e.conversationId === conversationId && e.threadTs === threadTs && e.comment !== undefined &&
    !stored.some(m => (e.sentTs && m.ts === e.sentTs) || (e.fileId && m.files.some(f => f.id === e.fileId))))
    .map(e => ({
      ts: e.ts, user_id: me, author: null, text: escapeText(e.comment ?? ""), blocks: e.blocks, subtype: null,
      reply_count: 0, repliers: [], latest_reply: null, edited: false, hidden_by_limit: false,
      reactions: null, thread_ts: e.threadTs, files: [], unfurls: [], client_msg_id: e.id,
      pending: e.state === "failed" ? "failed" : e.state === "accepted" ? "sent" : "sending",
      upload_name: e.file.name, upload_progress: e.state === "sending" ? uploadPercent(e.progress) : undefined,
      upload_uncertain: e.uncertain, error: e.error,
    }));
}

const [errors, setErrors] = createStore<Record<string, string | undefined>>({});
const destinationKey = (conversationId: string, threadTs?: string | null) => `${conversationId}/${threadTs ?? ""}`;
export const attachmentError = (conversationId: string, threadTs?: string | null) => errors[destinationKey(conversationId, threadTs)];
export function queueAttachments(conversationId: string, threadTs: string | null, files: File[]) {
  setErrors(destinationKey(conversationId, threadTs), addAttachments(conversationId, threadTs, files) ?? undefined);
}
export function fileDragOver(event: DragEvent) {
  if (event.dataTransfer?.types.includes("Files")) { event.preventDefault(); event.dataTransfer.dropEffect = "copy"; }
}
export function dropFiles(event: DragEvent, conversationId: string, threadTs: string | null) {
  if (!event.dataTransfer?.types.includes("Files")) return;
  event.preventDefault(); event.stopPropagation();
  queueAttachments(conversationId, threadTs, Array.from(event.dataTransfer.files));
}
