// Typed wrappers over the Rust commands and events (crates/app/src).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Status =
  | { phase: "starting" }
  | { phase: "importing" }
  | { phase: "sign_in"; desktop: boolean; reason: string | null }
  | { phase: "signing_in" }
  | { phase: "ready"; team_id: string; team: string; user_id: string; user: string };

export type Progress =
  | { stage: "directory" }
  | { stage: "conversations"; done: number; total: number; current: string; deep: boolean }
  | { stage: "files"; done: number; total: number }
  | { stage: "idle" };

export type ConnectionState = { state: "connected" } | { state: "reconnecting"; in_secs: number };

type Ref = { conversation_id: string; ts: string };
export type Change =
  | { type: "message"; data: Ref }
  | { type: "message_deleted"; data: Ref }
  | { type: "reactions"; data: Ref }
  | { type: "read_marker"; data: { conversation_id: string } }
  | { type: "typing"; data: { conversation_id: string; user_id: string } }
  | { type: "history"; data: { conversation_id: string } }
  | { type: "directory" }
  | { type: "connection"; data: ConnectionState }
  | { type: "progress"; data: Progress };

export type Conversation = {
  id: string;
  kind: "public_channel" | "private_channel" | "mpim" | "im";
  label: string;
  is_archived: boolean;
  topic: string | null;
  messages: number;
  unread: number | null;
  latest_ts: string | null;
  /// The other person of a DM, for their avatar.
  dm_user_id: string | null;
};

export type FileRef = {
  id: string;
  name: string | null;
  title: string | null;
  mimetype: string | null;
  filetype: string | null;
  size: number | null;
  local_path: string | null;
  permalink: string | null;
  hidden_by_limit: boolean;
  width: number | null;
  height: number | null;
  preview_path: string | null;
  preview_width: number | null;
  preview_height: number | null;
};

export type Unfurl = {
  service: string | null;
  title: string | null;
  title_link: string | null;
  text: string | null;
  author: string | null;
  footer: string | null;
  url: string | null;
  color: string | null;
  /// Hash of a cached image (see linkImageUrl).
  image: string | null;
  image_width: number | null;
  image_height: number | null;
  thumb: boolean;
  icon: string | null;
  video: boolean;
  fields: [string, string][];
};

export type Message = {
  ts: string;
  user_id: string | null;
  author: string | null;
  text: string;
  subtype: string | null;
  reply_count: number;
  /// Thread roots: distinct repliers, most recent first (up to 4).
  repliers: string[];
  /// Thread roots: ts of the newest reply.
  latest_reply: string | null;
  edited: boolean;
  hidden_by_limit: boolean;
  reactions: string | null;
  thread_ts: string | null;
  files: FileRef[];
  unfurls: Unfurl[];
};

/// A search result (core `store::search::Hit`). The snippet marks matched
/// terms with MARK_OPEN … MARK_CLOSE (see snippet.ts).
export type Hit = {
  conversation_id: string;
  conversation: string;
  ts: string;
  thread_ts: string | null;
  user_id: string | null;
  author: string | null;
  snippet: string;
  hidden_by_limit: boolean;
};

/// Workspace emoji (core `store::emoji::CustomEmoji`): `name → cached image
/// hash` (custom aliases resolved) and `name → standard emoji name`.
export type CustomEmoji = {
  images: Record<string, string>;
  aliases: Record<string, string>;
};

export const api = {
  status: () => invoke<Status>("status"),
  progress: () => invoke<Progress | null>("progress"),
  conversations: () => invoke<Conversation[]>("conversations"),
  messages: (conversationId: string, before: string | null, limit: number) =>
    invoke<Message[]>("messages", { conversationId, before, limit }),
  message: (conversationId: string, ts: string) =>
    invoke<Message | null>("message", { conversationId, ts }),
  thread: (conversationId: string, ts: string) =>
    invoke<Message[]>("thread", { conversationId, ts }),
  importDesktop: () => invoke<void>("import_desktop"),
  signIn: () => invoke<void>("sign_in"),
  people: () => invoke<Record<string, string>>("people"),
  avatars: () => invoke<Record<string, string>>("avatars"),
  customEmoji: () => invoke<CustomEmoji>("custom_emoji"),
  openFile: (fileId: string) => invoke<void>("open_file", { fileId }),
  pdfData: (fileId: string) => invoke<ArrayBuffer>("pdf_data", { fileId }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  search: (query: string, recent: boolean, limit: number) =>
    invoke<Hit[]>("search", { query, recent, limit }),
};

export const onStatus = (f: (s: Status) => void): Promise<UnlistenFn> =>
  listen<Status>("flek://status", (e) => f(e.payload));

export const onChange = (f: (c: Change) => void): Promise<UnlistenFn> =>
  listen<Change>("flek://change", (e) => f(e.payload));

/// A thread reply that is not also shown in the channel.
export const isReplyOnly = (m: Message) =>
  m.thread_ts !== null && m.thread_ts !== m.ts && m.subtype !== "thread_broadcast";

export const tsDate = (ts: string) => new Date(parseFloat(ts) * 1000);

/// URL of something the app's `flekfile` protocol serves
/// (crates/app/src/files.rs). WebView2 maps custom schemes to http.
let cacheUrl = (path: string) =>
  (navigator.userAgent.includes("Windows") ? "http://flekfile.localhost/" : "flekfile://localhost/") + path;

/// A cached file, or its preview (first page of a PDF, a video frame).
export const fileUrl = (id: string, preview = false) =>
  cacheUrl(encodeURIComponent(id) + (preview ? "/preview" : ""));

/// A cached link-preview image.
export const linkImageUrl = (hash: string) => cacheUrl("link/" + encodeURIComponent(hash));

/// For preview.html, which runs without the app.
export function overrideCacheUrl(f: (path: string) => string) {
  cacheUrl = f;
}
