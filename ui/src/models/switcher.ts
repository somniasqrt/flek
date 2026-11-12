import type { Conversation, DirectMessage } from "../api";
import { hasUnread } from "./conversation-model.ts";
import type { ShortcutEvent } from "./shortcuts.ts";
import { primaryModifier } from "./shortcuts.ts";

const normalize = (s: string) => s.normalize("NFKD").toLowerCase().replace(/\p{M}/gu, "");
function match(name: string, query: string): number {
  const value = normalize(name);
  if (value === query) return 0;
  if (value.startsWith(query)) return 1;
  if (value.split(/[\s,._-]+/).some((word) => word.startsWith(query))) return 2;
  if (value.includes(query)) return 3;
  let at = 0;
  for (const c of value) if (c === query[at]) at++;
  return at === query.length ? 4 : Infinity;
}

export function rankConversations(
  rows: readonly Conversation[],
  query: string,
  dms: readonly DirectMessage[] = [],
  people: Record<string, string> = {},
): Conversation[] {
  const q = normalize(query.trim().replace(/^[#@]/, ""));
  const members = new Map(dms.map((dm) => [dm.id, dm.members]));
  return rows
    .filter((c) => !c.is_archived)
    .map((c) => {
      const names = [
        c.label,
        ...(members.get(c.id) ?? [c.dm_user_id])
          .filter((id): id is string => !!id)
          .map((id) => people[id] ?? id),
      ];
      return { c, score: q ? Math.min(...names.map((name) => match(name, q))) : 0 };
    })
    .filter(({ score }) => Number.isFinite(score))
    .sort(
      (a, b) =>
        a.score - b.score ||
        Number(hasUnread(b.c)) - Number(hasUnread(a.c)) ||
        Number(b.c.latest_ts ?? 0) - Number(a.c.latest_ts ?? 0) ||
        a.c.label.localeCompare(b.c.label),
    )
    .map(({ c }) => c);
}

// Works on the full sidebar order, independently of section collapse state.
export function adjacentConversation(
  rows: readonly Conversation[],
  selected: string | null,
  direction: -1 | 1,
  unread = false,
): Conversation | undefined {
  if (!rows.length) return;
  const found = rows.findIndex((c) => c.id === selected);
  const index = found < 0 ? (direction === 1 ? -1 : 0) : found;
  for (let step = 1; step <= rows.length; step++) {
    const i = (index + direction * step + rows.length * 2) % rows.length;
    if (rows[i].id !== selected && (!unread || hasUnread(rows[i]))) return rows[i];
  }
}

export function conversationShortcut(e: ShortcutEvent, mac: boolean, editing: boolean) {
  if (e.isComposing) return;
  if (!e.altKey && !e.shiftKey && primaryModifier(e, mac) && e.code === "KeyK") return "switcher";
  if (editing || !e.altKey || e.metaKey || e.ctrlKey) return;
  if (e.code === "ArrowUp") return e.shiftKey ? "previous-unread" : "previous";
  if (e.code === "ArrowDown") return e.shiftKey ? "next-unread" : "next";
}
