export function firstUnread<T extends { ts: string }>(
  messages: readonly T[], lastRead: string | null | undefined,
): string | null {
  if (!lastRead) return null;
  return messages.find((m) => m.ts > lastRead)?.ts ?? null;
}

export function conversationActions(kind: string) {
  const channel = kind === "public_channel" || kind === "private_channel";
  return {
    channel,
    details: channel ? "Channel details" : "View profile",
    dismiss: channel ? "Leave channel" : "Close conversation",
  };
}

export function hasUnread(c: { unread: number | null; notification?: string }) {
  return c.notification !== "muted" && (c.unread ?? 0) > 0;
}
