import type { ChannelRow } from "./api.ts";

export type Filters = {
  query: string;
  membership: "all" | "joined" | "not_joined";
  visibility: "all" | "public" | "private";
  archived: boolean;
  sort: "name" | "members" | "newest";
};

export function filterChannels(rows: ChannelRow[], filters: Filters, lang = "en"): ChannelRow[] {
  const q = filters.query.trim().toLowerCase();
  return rows.filter((row) => {
    if (!filters.archived && row.is_archived) return false;
    if (filters.membership === "joined" && !row.is_member) return false;
    if (filters.membership === "not_joined" && row.is_member) return false;
    if (filters.visibility === "public" && row.is_private) return false;
    if (filters.visibility === "private" && !row.is_private) return false;
    return !q || [row.name, row.topic, row.purpose]
      .some((text) => text?.toLowerCase().includes(q));
  }).sort((a, b) => {
    const names = a.name.localeCompare(b.name, lang) || a.id.localeCompare(b.id);
    if (filters.sort === "members") return (b.num_members ?? 0) - (a.num_members ?? 0) || names;
    if (filters.sort === "newest") return (b.created ?? 0) - (a.created ?? 0) || names;
    return names;
  });
}

// Live preview only. The core command validates again before creation.
export function normalizeChannelName(value: string): string {
  return value.toLowerCase().replace(/\p{White_Space}/gu, "-");
}

export function channelNameIssue(value: string): "required" | "length" | "characters" | null {
  if (!value) return "required";
  if ([...value].length > 80) return "length";
  if (!/^[\p{Alphabetic}0-9_-]+$/u.test(value)) return "characters";
  return null;
}
