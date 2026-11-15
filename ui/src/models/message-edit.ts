// Stored Slack content -> a safe composer draft. Never insert remote HTML.
import { record } from "./block-kit.ts";
import { escapeText } from "./compose.ts";
import { parse, type Inline, type Block } from "./mrkdwn.ts";
import { safeLink } from "./rich-compose.ts";

const attr = (s: string) => escapeText(s).replace(/"/g, "&quot;");
const atom = (key: string, id: string, label: string) =>
  `<span contenteditable="false" data-${key}="${attr(id)}">${escapeText(label)}</span>`;
const link = (url: string, label: string) => safeLink(url)
  ? `<a href="${attr(url)}">${escapeText(label)}</a>` : escapeText(label);
const styled = (html: string, style: unknown) => {
  const s = record(style);
  for (const [key, tag] of [["bold", "b"], ["italic", "i"], ["underline", "u"], ["strike", "s"], ["code", "code"]])
    if (s[key]) html = `<${tag}>${html}</${tag}>`;
  return html;
};

type ListNode = { tag: "ul" | "ol"; items: { html: string; children: ListNode[] }[] };
type EditRun = { html: string; tag?: "ul" | "ol"; indent?: number };
// Slack lists are flat runs with indentation. Rebuild actual nested lists so
// the existing composer serializer preserves depth without empty wrapper items.
function renderRuns(runs: EditRun[]): string {
  const parts: (string | ListNode)[] = [], stack: ListNode[] = [];
  for (const run of runs) {
    if (!run.tag) { parts.push(run.html); stack.length = 0; continue; }
    const depth = Math.min(3, Math.max(0, Math.trunc(run.indent ?? 0)), stack.length);
    const parent = depth ? stack[depth - 1].items.at(-1)!.children : parts;
    if (!stack[depth] || stack[depth].tag !== run.tag) {
      const node: ListNode = { tag: run.tag, items: [] };
      parent.push(node);
      stack[depth] = node;
    }
    stack.length = depth + 1;
    stack[depth].items.push({ html: run.html, children: [] });
  }
  const list = (n: ListNode): string => `<${n.tag}>${n.items.map((item) =>
    `<li>${item.html}${item.children.map(list).join("")}</li>`).join("")}</${n.tag}>`;
  return parts.map((p) => typeof p === "string" ? p : list(p)).join("");
}

export function messageEditDraft(
  text: string, blocks: unknown,
  names: Readonly<Record<string, string>> = {},
  channels: Readonly<Record<string, string>> = {},
): string | null {
  const richInline = (raw: unknown): string | null => {
    const n = record(raw);
    if (n.type === "text" && typeof n.text === "string") return styled(escapeText(n.text).replace(/\n/g, "<br>"), n.style);
    if (n.type === "link" && typeof n.url === "string") return styled(link(n.url, typeof n.text === "string" ? n.text : n.url), n.style);
    if (n.type === "user" && typeof n.user_id === "string") return atom("user-id", n.user_id, `@${names[n.user_id] ?? n.user_id}`);
    if (n.type === "channel" && typeof n.channel_id === "string") return atom("channel-id", n.channel_id, `#${channels[n.channel_id] ?? n.channel_id}`);
    if (n.type === "broadcast" && ["here", "channel", "everyone"].includes(String(n.range))) return atom("broadcast", String(n.range), `@${n.range}`);
    if (n.type === "emoji" && typeof n.name === "string") {
      const name = typeof n.skin_tone === "number" && n.skin_tone >= 2 && n.skin_tone <= 6 && !n.name.includes("::skin-tone-") ? `${n.name}::skin-tone-${n.skin_tone}` : n.name;
      return atom("emoji-name", name, `:${name}:`);
    }
    return null;
  };
  const rich = (raw: unknown, depth = 0): string | null => {
    if (depth > 7) return null;
    const n = record(raw);
    if (!Array.isArray(n.elements)) return richInline(raw);
    if (n.elements.length > 1000) return null;
    const sections = ["rich_text_section", "rich_text_quote", "rich_text_preformatted"];
    if (sections.includes(String(n.type)) && n.elements.some((e) => Array.isArray(record(e).elements))) return null;
    if (n.type === "rich_text_list" && (
      !["ordered", "bullet"].includes(String(n.style)) ||
      (n.indent !== undefined && (!Number.isInteger(n.indent) || Number(n.indent) < 0 || Number(n.indent) > 3)) ||
      n.elements.some((e) => record(e).type !== "rich_text_section")
    )) return null;
    if (n.type === "rich_text" && n.elements.some((e) => ![...sections, "rich_text_list"].includes(String(record(e).type)))) return null;
    const children = n.elements.map((e) => rich(e, depth + 1));
    if (children.some((v) => v === null)) return null;
    const content = children.join("");
    switch (n.type) {
      case "rich_text": {
        const runs: EditRun[] = [];
        for (let i = 0; i < n.elements.length; i++) {
          const child = record(n.elements[i]);
          if (child.type === "rich_text_list" && Array.isArray(child.elements)) {
            const tag = child.style === "ordered" ? "ol" : "ul";
            for (const item of child.elements) {
              const html = rich(item, depth + 1);
              if (html === null) return null;
              runs.push({ html, tag, indent: Math.min(3, Math.max(0, Number(child.indent) || 0)) });
            }
          } else runs.push({ html: children[i]! });
        }
        return renderRuns(runs);
      }
      case "rich_text_section": return `<div>${content || "<br>"}</div>`;
      case "rich_text_quote": return `<blockquote>${content}</blockquote>`;
      case "rich_text_preformatted": return `<pre>${content}</pre>`;
      case "rich_text_list": {
        return renderRuns(children.map((html) => ({ html: html!, tag: n.style === "ordered" ? "ol" : "ul", indent: 0 })));
      }
      default: return null;
    }
  };
  const inline = (nodes: Inline[]): string => nodes.map((n): string => {
    switch (n.t) {
      case "text": return escapeText(n.v);
      case "special": return /^@(here|channel|everyone)$/.test(n.v)
        ? atom("broadcast", n.v.slice(1), n.v) : escapeText(n.v);
      case "b": case "i": case "s": return `<${n.t}>${inline(n.c)}</${n.t}>`;
      case "code": return `<code>${escapeText(n.v)}</code>`;
      case "user": return atom("user-id", n.id, `@${names[n.id] ?? n.label ?? n.id}`);
      case "channel": return atom("channel-id", n.id, `#${channels[n.id] ?? n.label ?? n.id}`);
      case "emoji": return atom("emoji-name", n.name, `:${n.name}:`);
      case "link": return safeLink(n.url) ? `<a href="${attr(n.url)}">${n.label ? inline(n.label) : escapeText(n.url)}</a>` : escapeText(n.url);
    }
  }).join("");
  const mrkdwn = (nodes: Block[]): string => nodes.map((n): string => {
    switch (n.t) {
      case "p": return `<div>${n.lines.map(inline).join("<br>")}</div>`;
      case "h": return `<div>${inline(n.c)}</div>`;
      case "pre": return `<pre>${escapeText(n.v)}</pre>`;
      case "quote": return `<blockquote>${mrkdwn(n.c)}</blockquote>`;
      case "list": {
        return renderRuns(n.items.map((item) => ({ html: inline(item.c), tag: n.ordered ? "ol" : "ul", indent: item.level })));
      }
    }
  }).join("");
  if (blocks !== undefined && blocks !== null && !Array.isArray(blocks)) return null;
  let html: string;
  if (Array.isArray(blocks) && blocks.length) {
    if (blocks.length > 100) return null;
    if (blocks.some((b) => record(b).type !== "rich_text")) return null;
    const parts = blocks.map((b) => rich(b));
    if (parts.some((v) => v === null)) return null;
    html = parts.join("");
  } else html = mrkdwn(parse(text));
  return JSON.stringify({ version: 1, html });
}

export function messageActionKey(e: Pick<KeyboardEvent, "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing">) {
  if (e.isComposing || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
  if (e.code === "KeyE") return "edit";
  if (e.code === "Delete" || e.code === "Backspace") return "delete";
}
