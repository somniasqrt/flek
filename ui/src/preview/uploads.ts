// Synthetic external-upload lifecycle. No bytes leave this preview.
import { Channel } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import type { FileRef } from "../api";
import { uploadFileError, UPLOAD_CHUNK_BYTES, type UploadProgress } from "../models/upload-model";
import { msg, ROOT, messages, replies, bigMessages } from "./messages";
import { elsewhere } from "./directory";

type Staged = { name: string; size: number; offset: number; cancelled: boolean; attempts: number; receipt?: unknown };
const staged = new Map<string, Staged>();
let next = 0;
export async function uploadIPC(command: string, a: Record<string, unknown>) {
  if (command === "prepare_upload") {
    const name = String(a.name), size = Number(a.size);
    const error = uploadFileError({ name, size });
    if (error) throw error;
    if (staged.size >= 10) throw "upload_queue_full";
    const id = `preview-upload-${++next}`;
    staged.set(id, { name, size, offset: 0, cancelled: false, attempts: 0 });
    return id;
  }
  const entry = staged.get(String(a.id));
  if (!entry || entry.cancelled) throw "upload_unavailable";
  if (command === "discard_upload") { entry.cancelled = true; staged.delete(String(a.id)); return; }
  if (command === "append_upload") {
    const bytes = a.bytes as number[], offset = Number(a.offset);
    if (!bytes.length || bytes.length > UPLOAD_CHUNK_BYTES || offset > entry.offset || offset + bytes.length > entry.size) throw "invalid upload chunk";
    entry.offset = Math.max(entry.offset, offset + bytes.length);
    return;
  }
  if (entry.receipt) return entry.receipt;
  if (entry.offset !== entry.size) throw "upload is not fully staged";
  const destination = a.destination as { conversation_id: string; thread_ts: string | null; comment: string; blocks?: unknown };
  // mockIPC passes a Channel object directly; real IPC serializes it.
  const channel = a.onProgress as Channel<UploadProgress>;
  const progress = (stage: UploadProgress["stage"], sent: number) => channel.onmessage({ stage, sent, total: entry.size, file_id: `FPREVIEW${a.id}` });
  for (const sent of [0, Math.floor(entry.size / 3), Math.floor(entry.size * 2 / 3), entry.size]) {
    if (entry.cancelled) throw "upload_cancelled";
    progress("uploading", sent);
    await new Promise(r => setTimeout(r, 200));
  }
  if (entry.cancelled) throw "upload_cancelled";
  if (entry.name.includes("fail") && entry.attempts++ === 0) throw "synthetic upload failure";
  progress("completing", entry.size);
  await new Promise(r => setTimeout(r, 250));
  const ts = `${Math.floor(Date.now() / 1000)}.${String(++next).padStart(6, "0")}`;
  const file: FileRef = {
    id: `FPREVIEW${a.id}`, name: entry.name, title: entry.name, size: entry.size,
    mimetype: "application/octet-stream", filetype: null, local_path: null,
    permalink: null, hidden_by_limit: false, width: null, height: null,
    preview_path: null, preview_width: null, preview_height: null,
  };
  const conv = destination.conversation_id;
  const m = msg(0, "U3", destination.comment.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"), {
    ts, files: [file], thread_ts: destination.thread_ts, subtype: "file_share",
    blocks: destination.blocks ? JSON.parse(JSON.stringify(destination.blocks)) : undefined,
  });
  if (conv === "CBIG") bigMessages.push(m);
  else if (conv !== "C1") elsewhere.push({ ...m, conversation_id: conv });
  else if (destination.thread_ts === ROOT) {
    replies.push(m);
    const root = messages.find(x => x.ts === ROOT);
    if (root) { root.reply_count = replies.length; root.latest_reply = ts; }
  } else messages.push(m);
  progress("syncing", entry.size);
  const uncertain = entry.name.includes("uncertain");
  if (!uncertain) await emit("flek://change", { type: "message", data: { conversation_id: conv, ts } });
  entry.receipt = { file_id: file.id, conversation_id: conv, ts: uncertain ? null : ts, not_stored_locally: uncertain, completion_unknown: uncertain };
  return entry.receipt;
}
