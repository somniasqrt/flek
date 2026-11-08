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
};

export type Message = {
  ts: string;
  user_id: string | null;
  author: string | null;
  text: string;
  subtype: string | null;
  reply_count: number;
  edited: boolean;
  hidden_by_limit: boolean;
  reactions: string | null;
  thread_ts: string | null;
  files: FileRef[];
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
  openFile: (fileId: string) => invoke<void>("open_file", { fileId }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export const onStatus = (f: (s: Status) => void): Promise<UnlistenFn> =>
  listen<Status>("flek://status", (e) => f(e.payload));

export const onChange = (f: (c: Change) => void): Promise<UnlistenFn> =>
  listen<Change>("flek://change", (e) => f(e.payload));

/// A thread reply that is not also shown in the channel.
export const isReplyOnly = (m: Message) =>
  m.thread_ts !== null && m.thread_ts !== m.ts && m.subtype !== "thread_broadcast";

export const tsDate = (ts: string) => new Date(parseFloat(ts) * 1000);
