import type { Conversation } from "../api";
import { personCompletions, type ComposerPerson } from "./composer-autocomplete.ts";

export const MAX_DM_PEOPLE = 8;
const MAX_ROWS = 60;

export type SelectedPerson = {
  id: string;
  label: string;
};

export type NewMessageRow =
  | { kind: "person"; id: string; label: string; detail: string }
  | { kind: "conversation"; id: string; label: string; conversationKind: Conversation["kind"] };

export function newMessageRows(
  query: string,
  selected: readonly SelectedPerson[],
  people: readonly ComposerPerson[],
  conversations: readonly Conversation[],
  me?: string,
): NewMessageRow[] {
  if (selected.length >= MAX_DM_PEOPLE) return [];

  const trimmed = query.trim();
  const peopleOnly = trimmed.startsWith("@");
  const channelsOnly = trimmed.startsWith("#");
  const q = trimmed.replace(/^[@#]/, "");
  const qLower = q.toLowerCase();

  const selectedIds = new Set(selected.map((p) => p.id));
  const eligiblePeople = people.filter((p) => {
    if (p.is_bot) return false;
    if (selected.length > 0 && (selectedIds.has(p.id) || (me !== undefined && p.id === me))) {
      return false;
    }
    return true;
  });

  const personRows: NewMessageRow[] = channelsOnly
    ? []
    : personCompletions(q, eligiblePeople, false).flatMap((c) =>
        c.kind === "person"
          ? [{ kind: "person" as const, id: c.id, label: c.label, detail: c.detail }]
          : [],
      );

  if (selected.length > 0) {
    return personRows.slice(0, MAX_ROWS);
  }

  if (peopleOnly) {
    return personRows.slice(0, MAX_ROWS);
  }

  const shownPersonIds = new Set(personRows.map((p) => p.id));
  const shownPersonLabels = new Set(personRows.map((p) => p.label.toLowerCase()));

  const conversationRows: NewMessageRow[] = conversations
    .filter((c) => {
      if (c.is_archived) return false;
      if (channelsOnly && !c.kind.endsWith("channel")) return false;
      if (!c.label.toLowerCase().includes(qLower)) return false;
      if (c.kind === "im") {
        if (c.dm_user_id) {
          if (shownPersonIds.has(c.dm_user_id)) return false;
        } else if (shownPersonLabels.has(c.label.toLowerCase())) {
          return false;
        }
      }
      return true;
    })
    .map((c) => ({
      kind: "conversation" as const,
      id: c.id,
      label: c.label,
      conversationKind: c.kind,
    }));

  return [...conversationRows, ...personRows].slice(0, MAX_ROWS);
}
