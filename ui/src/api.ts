// Typed wrappers over the Rust commands and events (crates/app/src).
import type { Presence, CustomStatus } from "./presence";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Status =
  | { phase: "starting" }
  | { phase: "importing" }
  | { phase: "sign_in"; desktop: boolean; reason: string | null }
  | { phase: "signing_in" }
  | {
      phase: "ready";
      team_id: string;
      team: string;
      user_id: string;
      user: string;
    };

export type Progress =
  | { stage: "directory" }
  | {
      stage: "conversations";
      done: number;
      total: number;
      current: string;
      deep: boolean;
    }
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
  | { type: "presence" }
  | { type: "connection"; data: ConnectionState }
  | { type: "progress"; data: Progress };

export type Conversation = {
  last_read?: string | null;
  starred?: boolean;
  notification?: "all" | "mentions" | "muted";
  is_general?: boolean;
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
  own_reactions?: string[];
  blocks?: unknown;
  thread_ts: string | null;
  files: FileRef[];
  unfurls: Unfurl[];
  /// Id the sending client chose (matches an optimistic copy).
  client_msg_id?: string | null;
  /// UI only: an optimistic copy not stored yet (see outbox.ts).
  pending?: "sending" | "failed";
  /// UI only: why a pending copy failed.
  error?: string;
};

/// A message Slack accepted and Flek stored (core `sync::send::Sent`).
export type Sent = {
  conversation_id: string;
  ts: string;
  client_msg_id: string;
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

/// A person, for the profile panel (core `store::directory::Profile`).
/// Email and phone are personal data: shown, never logged.
export type Profile = {
  id: string;
  name: string;
  real_name: string | null;
  display_name: string | null;
  title: string | null;
  email: string | null;
  phone: string | null;
  pronouns: string | null;
  status_text: string | null;
  status_emoji: string | null;
  status_expiration: number | null;
  /// IANA zone, e.g. "Europe/Kyiv".
  tz: string | null;
  tz_label: string | null;
  /// Seconds east of UTC.
  tz_offset: number | null;
  is_bot: boolean;
  deleted: boolean;
  /// Cached 72 px avatar hash (see linkImageUrl).
  avatar: string | null;
  /// Cached large photo hash, if fetched already.
  photo: string | null;
  has_photo: boolean;
  /// The DM with this person, if Flek has it.
  dm_conversation_id: string | null;
};

export type DirectMessage = {
  id: string;
  label: string;
  kind: string;
  partner_id: string | null;
  members: string[];
  unread: number | null;
  ts: string | null;
  user_id: string | null;
  text: string;
  file_name: string | null;
  hidden_by_limit: boolean;
};

export type ActivityItem = {
  kind: "dm" | "mention" | "thread" | "reaction";
  conversation_id: string;
  ts: string;
  event_ts: string;
  thread_ts: string | null;
  user_id: string | null;
  text: string;
  file_name: string | null;
  hidden_by_limit: boolean;
  reaction: string | null;
  unread: boolean;
  unread_count: number;
};

export type ActivityPage = {
  items: ActivityItem[];
  unread_count: number;
  has_more: boolean;
};

export type StoredFile = {
  title: string | null;
  id: string;
  name: string | null;
  mimetype: string | null;
  size: number | null;
  local_path: string | null;
  conversation_id: string | null;
  ts: string | null;
  user_id: string | null;
  hidden_by_limit: boolean;
  permalink: string | null;
};

export type ChannelInfo = {
  conversation: {
    id: string;
    name: string | null;
    label: string;
    topic: string | null;
    purpose: string | null;
  };
  created: number | null;
  creator: string | null;
  num_members: number | null;
  members: { id: string; name: string; display_name: string | null }[] | null;
  pins: { ts: string; text: string }[] | null;
};

export type StorageInfo = { db_path: string; db_bytes: number; cache_bytes: number };

export const api = {
  getSettings: () => invoke<import("./settings-model").Settings>("get_settings"),
  setSettings: (settings: import("./settings-model").Settings) =>
    invoke<import("./settings-model").Settings>("set_settings", { settings }),
  storageInfo: () => invoke<StorageInfo>("storage_info"),
  showStorage: () => invoke<void>("show_storage"),
  clearFileCache: () => invoke<void>("clear_file_cache"),
  fullResync: () => invoke<void>("full_resync"),
  signOut: () => invoke<void>("sign_out"),
  accountOrigin: () => invoke<string>("account_origin"),
  markRead: (conversationId: string, ts: string) =>
    invoke<void>("mark_read", { conversationId, ts }),
  conversationPreference: (
    conversationId: string,
    starred: boolean | null,
    notification: string | null,
  ) => invoke<void>("conversation_preference", { conversationId, starred, notification }),
  dismissConversation: (conversationId: string) =>
    invoke<void>("dismiss_conversation", { conversationId }),
  channelInfo: (conversationId: string, fresh = false) =>
    invoke<ChannelInfo>("channel_info", { conversationId, fresh }),
  conversationLink: (conversationId: string) =>
    invoke<string | null>("conversation_link", { conversationId }),
  teamIcon: () => invoke<string | null>("team_icon"),
  directMessages: () => invoke<DirectMessage[]>("direct_messages"),
  activity: (tab: string, unread: boolean, limit: number, offset: number) =>
    invoke<ActivityPage>("activity", { tab, unread, limit, offset }),
  browseFiles: (query: string, kind: string | null, sort: string, mine: boolean, offset: number) =>
    invoke<StoredFile[]>("browse_files", { query, kind, sort, mine, offset }),
  fetchFile: (fileId: string, reveal = false) => invoke<void>("fetch_file", { fileId, reveal }),
  presenceFor: (ids: string[]) => invoke<Presence[]>("presence_for", { ids }),
  statusesFor: (ids: string[]) => invoke<Record<string, CustomStatus>>("statuses_for", { ids }),
  subscribePresence: (ids: string[]) => invoke<void>("subscribe_presence", { ids }),
  setStatus: (status: CustomStatus) => invoke<void>("set_status", { status }),
  clearStatus: () => invoke<void>("clear_status"),
  setPresence: (away: boolean) => invoke<void>("set_presence", { away }),
  snooze: (minutes: number) => invoke<void>("snooze", { minutes }),
  endSnooze: () => invoke<void>("end_snooze"),
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
  profile: (userId: string) => invoke<Profile | null>("profile", { userId }),
  /// Fetches the large photo if missing; its cache hash (see linkImageUrl).
  profilePhoto: (userId: string) => invoke<string | null>("profile_photo", { userId }),
  openFile: (fileId: string) => invoke<void>("open_file", { fileId }),
  pdfData: (fileId: string) => invoke<ArrayBuffer>("pdf_data", { fileId }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  search: (query: string, recent: boolean, limit: number) =>
    invoke<Hit[]>("search", { query, recent, limit }),
  setReaction: (conversationId: string, ts: string, name: string, on: boolean) =>
    invoke<void>("set_reaction", { conversationId, ts, name, on }),
  sendMessage: (
    conversationId: string,
    text: string,
    threadTs: string | null,
    broadcast: boolean,
    clientMsgId: string,
    blocks?: unknown[],
  ) =>
    invoke<Sent>("send_message", {
      conversationId,
      text,
      threadTs,
      broadcast,
      clientMsgId,
      blocks,
    }),
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
  (navigator.userAgent.includes("Windows")
    ? "http://flekfile.localhost/"
    : "flekfile://localhost/") + path;

/// A cached file, or its preview (first page of a PDF, a video frame).
export const fileUrl = (id: string, preview = false) =>
  cacheUrl(encodeURIComponent(id) + (preview ? "/preview" : ""));

/// A cached link-preview image.
export const linkImageUrl = (hash: string) => cacheUrl("link/" + encodeURIComponent(hash));

/// For preview.html, which runs without the app.
export function overrideCacheUrl(f: (path: string) => string) {
  cacheUrl = f;
}
