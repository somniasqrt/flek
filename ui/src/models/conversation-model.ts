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

export function conversationStarLabel(c: { kind: string; starred?: boolean }) {
  const channel = c.kind === "public_channel" || c.kind === "private_channel";
  return channel
    ? c.starred ? t("Unstar channel") : t("Star channel")
    : c.starred ? t("Unstar conversation") : t("Star conversation");
}

/// Header menus anchor their right edge; sidebar context menus anchor their left.
export function conversationMenuPosition(
  x: number, y: number, width: number, height: number,
  viewportWidth: number, viewportHeight: number, rightAligned: boolean,
) {
  return {
    left: Math.max(8, Math.min(rightAligned ? x - width : x, viewportWidth - width - 8)),
    top: Math.max(8, Math.min(y, viewportHeight - height - 8)),
  };
}
