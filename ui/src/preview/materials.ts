// Synthetic catalogue shares always point at messages in the preview timeline.
import type { Material, MaterialPage, MaterialSort, Message } from "../api";
import { bigMessages, conversations, messages, people, replies } from "./messages";
import { elsewhere } from "./directory";

// Enough genuine link-bearing preview messages to exercise cursor pagination.
for (let i = 1; i <= 125; i++) {
  const url = `https://example.com/material/${i}`;
  if (!bigMessages[i].text.includes(url)) bigMessages[i].text += ` <${url}|Practice material ${i}>`;
}

function catalogue(): Material[] {
  const rows: Material[] = [];
  const add = (conversation_id: string, m: Message) => {
    const base = {
      conversation_id, conversation: conversations.find((c) => c.id === conversation_id)?.label ?? conversation_id,
      ts: m.ts, thread_ts: m.thread_ts, user_id: m.user_id,
      author: m.user_id ? people[m.user_id] ?? m.user_id : m.author,
      snippet: m.text, hidden_by_limit: m.hidden_by_limit,
    };
    const links = new Map<string, string>();
    for (const match of m.text.matchAll(/https?:\/\/[^\s<>|"]+/g)) {
      const url = match[0];
      const label = m.text.slice(match.index! + url.length).match(/^\|([^>]+)>/)?.[1];
      links.set(url, label ?? url);
    }
    for (const [url, title] of links) rows.push({ ...base, kind: "link", key: url, title, url });
    for (const file of m.files) rows.push({ ...base, kind: "file", key: file.id, title: file.name ?? file.title ?? file.id, url: file.permalink });
  };
  const seen = new Set<string>();
  for (const [id, m] of [
    ...messages.map((m) => ["C1", m] as const), ...replies.map((m) => ["C1", m] as const),
    ...bigMessages.map((m) => ["CBIG", m] as const), ...elsewhere.map((m) => [m.conversation_id, m] as const),
  ]) {
    const key = `${id}:${m.ts}`;
    if (!seen.has(key)) { seen.add(key); add(id, m); }
  }
  return rows;
}

export function materialPage(query: string, sort: MaterialSort, limit: number, cursor: string | null): MaterialPage {
  const tokens = query.match(/(?:[^\s"]+|"[^"]*")+/g) ?? [];
  const channels: string[] = [], authors: string[] = [], words: string[] = [];
  let after = "", before = "", link = false, file = false;
  for (const token of tokens) {
    const [key, ...parts] = token.split(":");
    const value = parts.join(":").replace(/^[#@]/, "").toLowerCase();
    if (key === "in") channels.push(value);
    else if (key === "from") authors.push(value);
    else if (key === "has") { if (value === "link") link = true; else if (value === "file") file = true; else throw "unknown has filter"; }
    else if (key === "before" || key === "after") {
      const day = new Date(`${value}T00:00:00Z`);
      if (!/^\d{4}-\d{2}-\d{2}$/.test(value) || !Number.isFinite(day.getTime()) || day.toISOString().slice(0, 10) !== value) throw "bad date, expected YYYY-MM-DD";
      if (key === "before") before = String(day.getTime() / 1000);
      else after = String(day.getTime() / 1000 + 86400);
    } else words.push(token.toLowerCase().replaceAll('"', "").replace(/\*$/, ""));
  }
  const list = catalogue().filter((m) =>
    (!channels.length || channels.includes(m.conversation.toLowerCase()) || channels.includes(m.conversation_id.toLowerCase())) &&
    (!authors.length || authors.includes(m.user_id?.toLowerCase() ?? "") || authors.includes(m.author?.toLowerCase() ?? "")) &&
    (!before || Number(m.ts) < Number(before)) && (!after || Number(m.ts) >= Number(after)) &&
    (link === file || m.kind === (link ? "link" : "file")) &&
    words.every((word) => { const haystack = `${m.title} ${m.url} ${m.snippet}`.toLowerCase(); return word.startsWith("-") ? !haystack.includes(word.slice(1)) : haystack.includes(word); }),
  );
  const identity = (m: Material) => `${m.conversation_id}:${m.ts}:${m.kind}:${m.key}`;
  list.sort((a, b) => (sort === "name" ? a.title.toLowerCase().localeCompare(b.title.toLowerCase()) :
    sort === "oldest" ? a.ts.localeCompare(b.ts) : b.ts.localeCompare(a.ts)) || identity(a).localeCompare(identity(b)));
  const page = cursor ? JSON.parse(cursor) as { query: string; sort: string; offset: number } : { query, sort, offset: 0 };
  if (page.query !== query || page.sort !== sort) throw "materials cursor belongs to another search";
  const end = page.offset + limit;
  return { items: list.slice(page.offset, end).map((m) => ({ ...m, snippet: "" })), next_cursor: end < list.length ? JSON.stringify({ query, sort, offset: end }) : null };
}
