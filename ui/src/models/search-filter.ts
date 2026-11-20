// Completion for the `in:` and `from:` search filters. Pure; the search bar
// owns the input element and the people/conversation lists.
import { channelCompletions, personCompletions, type ComposerPerson } from "./composer-autocomplete.ts";

export type FilterToken = { kind: "in" | "from"; query: string; start: number; end: number };
export type FilterSuggestion = {
  kind: "channel" | "dm" | "person";
  id: string;
  label: string;
  detail: string;
  private: boolean;
  /// The filter text that replaces the token, with a trailing space.
  insert: string;
};
type ScopeConversation = {
  id: string;
  kind: "public_channel" | "private_channel" | "mpim" | "im";
  label: string;
  is_archived: boolean;
  is_member?: boolean;
};

/// The filter the caret is in, e.g. `in:#gen|` or `from:al|ex`. The token runs
/// to the next whitespace on both sides, so accepting replaces all of it.
export function detectFilter(text: string, caret: number): FilterToken | null {
  const before = text.slice(0, caret);
  const m = /(?:^|\s)(in|from):([^\s"]*)$/i.exec(before);
  if (!m) return null;
  const after = /^[^\s]*/.exec(text.slice(caret))![0];
  const start = caret - m[1].length - 1 - m[2].length;
  return {
    kind: m[1].toLowerCase() as "in" | "from",
    query: m[2] + after,
    start,
    end: caret + after.length,
  };
}

/// A handle is readable but can collide with someone else's display or real
/// name (core would then report an ambiguous person); the id never does.
function personFilter(p: ComposerPerson, all: ComposerPerson[]): string {
  const handle = p.name.toLowerCase();
  const clash = /\s/.test(p.name) ||
    all.some(
      (o) =>
        o.id !== p.id &&
        [o.name, o.display_name, o.real_name].some((a) => a?.toLowerCase() === handle),
    );
  return clash ? p.id : `@${p.name}`;
}

/// Same rule as the ⌘F prefill: channels by name, DMs and names with spaces by id.
function scopeFilterText(c: Pick<ScopeConversation, "id" | "kind" | "label">): string {
  return c.kind === "im" || c.kind === "mpim" || /\s/.test(c.label) ? c.id : `#${c.label}`;
}

export function filterSuggestions(
  token: FilterToken,
  conversations: ScopeConversation[],
  people: ComposerPerson[],
  limit = 6,
): FilterSuggestion[] {
  const sigil = token.query[0];
  const q = token.query.replace(/^[#@]/, "");
  if (token.kind === "from") {
    if (sigil === "#") return [];
    return personCompletions(q, people, false)
      .slice(0, limit)
      .flatMap((c) => {
        const p = people.find((x) => x.id === c.id);
        if (!p || c.kind !== "person") return [];
        return [{ kind: "person", id: p.id, label: c.label, detail: c.detail, private: false, insert: `from:${personFilter(p, people)} ` }];
      });
  }
  const shape = (c: ScopeConversation) => ({
    id: c.id,
    name: c.label,
    is_private: c.kind !== "public_channel",
    is_member: c.is_member ?? true,
    is_archived: c.is_archived,
  });
  const byId = new Map(conversations.map((c) => [c.id, c]));
  const pick = (kinds: ScopeConversation["kind"][]) =>
    channelCompletions(q, conversations.filter((c) => kinds.includes(c.kind)).map(shape)).map((s) => byId.get(s.id)!);
  const channels = sigil === "@" ? [] : pick(["public_channel", "private_channel"]);
  const dms = sigil === "#" ? [] : pick(["im", "mpim"]);
  return [...channels, ...dms].slice(0, limit).map((c) => ({
    kind: c.kind === "im" || c.kind === "mpim" ? "dm" : "channel",
    id: c.id,
    label: c.label,
    detail: "",
    private: c.kind === "private_channel",
    insert: `in:${scopeFilterText(c)} `,
  }));
}

/// Replaces the token and returns the new text with the caret after the filter.
export function applyFilter(text: string, token: FilterToken, insert: string): { text: string; caret: number } {
  const rest = text.slice(token.end).replace(/^\s+/, "");
  const head = text.slice(0, token.start) + insert;
  return { text: head + rest, caret: head.length };
}
