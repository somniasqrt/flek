// Pure trigger and ranking rules; DOM selection and IPC live in the composer.
export type CompletionToken = {
  kind: "person" | "channel" | "emoji";
  query: string;
  start: number;
  end: number;
  complete: boolean;
};
export type ComposerPerson = {
  id: string;
  name: string;
  display_name: string | null;
  real_name: string | null;
  is_bot: boolean;
  is_member: boolean;
  recent_dm: boolean;
};
export type Completion =
  | { kind: "person"; id: string; label: string; detail: string }
  | { kind: "broadcast"; id: "here" | "channel" | "everyone"; label: string }
  | { kind: "channel"; id: string; label: string; private: boolean }
  | { kind: "emoji"; id: string; label: string };

export function detectCompletion(text: string, inCode = false): CompletionToken | null {
  if (inCode) return null;
  // Typed mrkdwn code has not become CODE/PRE DOM yet. Ignore its contents too.
  let fence = false,
    inline = 0;
  for (const m of text.matchAll(/`+|\n/g)) {
    if (m[0] === "\n") {
      inline = 0;
      continue;
    }
    if (m[0].length >= 3) {
      fence = !fence;
      inline = 0;
    } else if (!fence) inline = inline === m[0].length ? 0 : inline || m[0].length;
  }
  if (fence || inline) return null;
  const match = /(?:^|[\s([{“‘"'])([@#:])([\p{L}\p{N}_+.'-]*)(:?)$/u.exec(text);
  if (!match) return null;
  const [, trigger, query, closing] = match;
  if (trigger !== ":" && closing) return null;
  if (trigger === ":" && query.length < (closing ? 1 : 2)) return null;
  return {
    kind: trigger === "@" ? "person" : trigger === "#" ? "channel" : "emoji",
    query,
    start: text.length - trigger.length - query.length - closing.length,
    end: text.length,
    complete: !!closing,
  };
}

const matchRank = (values: string[], query: string) => {
  const q = query.toLowerCase();
  const normalized = values.map((v) => v.toLowerCase());
  return normalized.some((v) => v.startsWith(q))
    ? 0
    : normalized.some((v) => v.includes(q))
      ? 1
      : -1;
};
export function personCompletions(
  query: string,
  people: ComposerPerson[],
  broadcasts: boolean,
): Completion[] {
  const rows = people
    .map((p) => ({
      p,
      rank: matchRank([p.display_name || "", p.real_name || "", p.name].filter(Boolean), query),
    }))
    .filter(({ p, rank }) => rank >= 0 && (!p.is_bot || !!query))
    .sort(
      (a, b) =>
        a.rank - b.rank ||
        Number(b.p.is_member) - Number(a.p.is_member) ||
        Number(b.p.recent_dm) - Number(a.p.recent_dm) ||
        (a.p.display_name || a.p.real_name || a.p.name).localeCompare(
          b.p.display_name || b.p.real_name || b.p.name,
        ),
    )
    .map(
      ({ p }): Completion => ({
        kind: "person",
        id: p.id,
        label: p.display_name || p.real_name || p.name,
        detail: p.real_name || p.name,
      }),
    );
  if (broadcasts)
    for (const id of ["here", "channel", "everyone"] as const)
      if (matchRank([id], query) >= 0) rows.push({ kind: "broadcast", id, label: id });
  return rows;
}
export function channelCompletions(
  query: string,
  channels: {
    id: string;
    name: string;
    is_private: boolean;
    is_member: boolean;
    is_archived: boolean;
  }[],
): Completion[] {
  return channels
    .map((c) => ({ c, rank: matchRank([c.name], query) }))
    .filter(({ c, rank }) => !c.is_archived && rank >= 0)
    .sort(
      (a, b) =>
        Number(b.c.is_member) - Number(a.c.is_member) ||
        a.rank - b.rank ||
        a.c.name.localeCompare(b.c.name),
    )
    .map(({ c }) => ({ kind: "channel", id: c.id, label: c.name, private: c.is_private }));
}
export function emojiCompletions(
  query: string,
  names: string[],
  frequency: Readonly<Record<string, number>> = {},
): Completion[] {
  const usage: Record<string, number> = Object.create(null);
  for (const [name, count] of Object.entries(frequency)) {
    const base = name.split("::")[0];
    usage[base] = (usage[base] || 0) + count;
  }
  return [...new Set(names)]
    .map((name) => ({ name, rank: matchRank([name], query) }))
    .filter((e) => e.rank >= 0)
    .sort(
      (a, b) =>
        (usage[b.name] || 0) - (usage[a.name] || 0) ||
        a.rank - b.rank ||
        a.name.localeCompare(b.name),
    )
    .map(({ name }) => ({ kind: "emoji", id: name, label: name }));
}
