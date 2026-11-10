import { t } from "../i18n/index.ts";
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
    details: channel ? t("Channel details") : t("View profile"),
    dismiss: channel ? t("Leave channel") : t("Close conversation"),
  };
}

export function hasUnread(c: { unread: number | null; notification?: string }) {
  return c.notification !== "muted" && (c.unread ?? 0) > 0;
}
