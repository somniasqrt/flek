// Messages being sent. The database stays the source of truth: a pending
// entry only fills the gap until the stored message (matched by
// client_msg_id, or by ts once Slack answered) shows up in a list.
import { createStore, produce } from "solid-js/store";
import { api, type Message } from "./api";
import type { RichBlock } from "./rich-compose";
import { escapeText, newClientMsgId, nowTs } from "./compose";

type Entry = {
  client_msg_id: string;
  conversation_id: string;
  /// Root of the thread it replies in.
  thread_ts: string | null;
  broadcast: boolean;
  text: string;
  blocks?: RichBlock[];
  /// Where it sits in a list until the stored copy replaces it.
  ts: string;
  state: "sending" | "failed" | "sent";
  /// Slack's ts, once accepted.
  sent_ts?: string;
  sent_at?: number;
  error?: string;
};

const [entries, setEntries] = createStore<Entry[]>([]);
let me: string | null = null;

/// The signed-in user: the author of optimistic copies.
export const setMe = (id: string) => (me = id);

/// Accepted entries are kept this long at most, waiting for their stored copy.
const SENT_TTL_MS = 120_000;

/// Queues `text` (as typed) for sending and returns its client id.
export function send(conversationId: string, text: string, threadTs: string | null, broadcast: boolean, blocks?: RichBlock[]) {
  const now = Date.now();
  const entry: Entry = {
    client_msg_id: newClientMsgId(),
    conversation_id: conversationId,
    thread_ts: threadTs,
    broadcast: !!threadTs && broadcast,
    text,
    blocks,
    ts: nowTs(now),
    state: "sending",
  };
  setEntries((list) => [
    ...list.filter((e) => e.state !== "sent" || now - (e.sent_at ?? now) < SENT_TTL_MS),
    entry,
  ]);
  deliver(entry.client_msg_id);
  return entry.client_msg_id;
}

const update = (id: string, f: (e: Entry) => void) =>
  setEntries(
    (e) => e.client_msg_id === id,
    produce((e) => f(e)),
  );

async function deliver(id: string) {
  const e = entries.find((x) => x.client_msg_id === id);
  if (!e) return;
  update(id, (x) => {
    x.state = "sending";
    x.error = undefined;
  });
  try {
    // The same client id on a retry, so it stays one message.
    const sent = await api.sendMessage(e.conversation_id, e.text, e.thread_ts, e.broadcast, id, e.blocks);
    update(id, (x) => {
      x.state = "sent";
      x.sent_ts = sent.ts;
      x.sent_at = Date.now();
    });
  } catch (err) {
    update(id, (x) => {
      x.state = "failed";
      x.error = String(err);
    });
  }
}

export const retry = (id: string) => deliver(id);

export const discard = (id: string) =>
  setEntries((list) => list.filter((e) => e.client_msg_id !== id));

/// Optimistic copies for a list, as messages: the channel timeline
/// (`threadTs` null: top-level messages and broadcasts) or one thread.
/// Entries whose stored message is already in `stored` are left out.
export function pendingFor(conversationId: string, threadTs: string | null, stored: readonly Message[]): Message[] {
  const out: Message[] = [];
  for (const e of entries) {
    if (e.conversation_id !== conversationId) continue;
    if (threadTs ? e.thread_ts !== threadTs : e.thread_ts && !e.broadcast) continue;
    const landed = stored.some(
      (m) => m.client_msg_id === e.client_msg_id || (e.sent_ts !== undefined && m.ts === e.sent_ts),
    );
    if (landed) continue;
    out.push({
      ts: e.ts,
      user_id: me,
      author: null,
      text: escapeText(e.text),
      blocks: e.blocks,
      subtype: e.broadcast ? "thread_broadcast" : null,
      reply_count: 0,
      repliers: [],
      latest_reply: null,
      edited: false,
      hidden_by_limit: false,
      reactions: null,
      thread_ts: e.thread_ts,
      files: [],
      unfurls: [],
      client_msg_id: e.client_msg_id,
      // Accepted but not in the list yet: still shown as sending.
      pending: e.state === "failed" ? "failed" : "sending",
      error: e.error,
    });
  }
  return out;
}
