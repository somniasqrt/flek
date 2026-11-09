// Pure helpers for the composer and the outbox (no Solid, no IPC: unit-tested).

/// Slack reserves `&`, `<` and `>` in message text; the core escapes them
/// the same way before sending. Used for the optimistic copy, so it renders
/// exactly like the stored message will.
export const escapeText = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/// What is actually sent: the typed text without surrounding blank space
/// (Slack trims it too), empty when there is nothing to send.
export const outgoingText = (s: string) => s.trim();

const DRAFT_PREFIX = "flek.draft.";

/// localStorage key of a draft: one per conversation, one per thread.
export const draftKey = (conversationId: string, threadTs?: string | null) =>
  DRAFT_PREFIX + conversationId + (threadTs ? "/" + threadTs : "");

export function readDraft(key: string): string {
  try {
    return localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

export function writeDraft(key: string, text: string) {
  try {
    if (text) localStorage.setItem(key, text);
    else localStorage.removeItem(key);
  } catch {
    // Drafts are a convenience; a blocked storage only loses them.
  }
}

/// A random UUID (v4) for `client_msg_id`.
export function newClientMsgId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  const b = Array.from({ length: 16 }, () => Math.floor(Math.random() * 256));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = b.map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

/// A Slack-style ts for "now", for placing an optimistic copy.
export const nowTs = (ms = Date.now()) => (ms / 1000).toFixed(6);
