// Slack mrkdwn (the `text` of a message) → a small tree the timeline renders.
//
// Slack escapes `&`, `<` and `>` in text, so every real `<…>` is a token:
// `<@U1|name>`, `<#C1|general>`, `<!here>`, `<https://x|label>`. Besides
// Slack's own syntax (*bold*, _italic_, ~strike~, `code`, ```blocks```,
// > quotes) common Markdown that people type is shown too: `#` headings and
// `-` / `•` / `1.` lists.

export type Inline =
  | { t: "text"; v: string }
  | { t: "b" | "i" | "s"; c: Inline[] }
  | { t: "code"; v: string }
  | { t: "link"; url: string; label?: Inline[] }
  | { t: "user"; id: string; label?: string }
  | { t: "channel"; id: string; label?: string }
  | { t: "special"; v: string }
  | { t: "emoji"; name: string };

export type ListItem = { level: number; c: Inline[] };

export type Block =
  | { t: "p"; lines: Inline[][] }
  | { t: "h"; level: number; c: Inline[] }
  | { t: "quote"; c: Block[] }
  | { t: "list"; ordered: boolean; start: number; items: ListItem[] }
  | { t: "pre"; v: string };

export const unescape = (s: string) =>
  s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

export function parse(text: string): Block[] {
  const blocks: Block[] = [];
  for (const part of fencedParts(text)) {
    if (part.code) blocks.push({ t: "pre", v: unescape(part.text) });
    else lineBlocks(part.text, blocks);
  }
  return blocks;
}

/// The message as plain text for the clipboard: the stored text, never the
/// rendered one, so shortened links keep their full address.
export function plainText(text: string, userName: (id: string) => string | undefined = () => undefined): string {
  const inlines = (list: Inline[]): string => list.map(one).join("");
  const one = (n: Inline): string => {
    switch (n.t) {
      case "text":
      case "code":
      case "special":
        return n.v;
      case "b":
      case "i":
      case "s":
        return inlines(n.c);
      case "user":
        return "@" + (userName(n.id) ?? n.label ?? n.id);
      case "channel":
        return "#" + (n.label ?? n.id);
      case "emoji":
        return `:${n.name}:`;
      case "link": {
        const url = n.url.startsWith("mailto:") ? n.url.slice(7) : n.url;
        const label = n.label ? inlines(n.label) : "";
        return !label || label === url || label === n.url ? url : `${label} (${url})`;
      }
    }
  };
  const blocks = (list: Block[]): string =>
    list
      .map((b) => {
        switch (b.t) {
          case "p":
            return b.lines.map(inlines).join("\n");
          case "h":
            return inlines(b.c);
          case "quote":
            return blocks(b.c).replace(/^/gm, "> ");
          case "list":
            return b.items
              .map((item, i) => "  ".repeat(item.level) + (b.ordered ? `${b.start + i}. ` : "• ") + inlines(item.c))
              .join("\n");
          case "pre":
            return b.v;
        }
      })
      .join("\n");
  return blocks(parse(text));
}

// Shared with the rich composer: preserve ordinary text, recognize closed fences only.
export function fencedParts(text: string): { code: boolean; text: string }[] {
  const parts = text.split("```");
  if (parts.length % 2 === 0) parts.push(parts.pop()!.replace(/^/, parts.pop()! + "```"));
  return parts.map((text, i) => ({
    code: i % 2 === 1,
    text: i % 2 === 1 ? text.replace(/^\n/, "").replace(/\n$/, "") : text,
  }));
}

export function trimUrl(url: string): string {
  while (/[.,;:!?)]$/.test(url)) {
    if (url.endsWith(")") && [...url].filter(c => c === "(").length >= [...url].filter(c => c === ")").length) break;
    url = url.slice(0, -1);
  }
  return url;
}

const QUOTE = /^(?:&gt;|>)\s?(.*)$/;
const HEADING = /^(#{1,3})\s+(.+)$/;
const BULLET = /^(\s*)([•◦▪‣\-*–])\s+(.+)$/;
const NUMBERED = /^(\s*)(\d{1,3})[.)]\s+(.+)$/;

function lineBlocks(text: string, out: Block[]) {
  const lines = text.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (QUOTE.test(line)) {
      const inner: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) inner.push(lines[i++].match(QUOTE)![1]);
      const c: Block[] = [];
      lineBlocks(inner.join("\n"), c);
      out.push({ t: "quote", c });
      continue;
    }
    const h = line.match(HEADING);
    if (h) {
      out.push({ t: "h", level: h[1].length, c: inline(h[2]) });
      i++;
      continue;
    }
    if (BULLET.test(line) || NUMBERED.test(line)) {
      const ordered = !BULLET.test(line);
      const re = ordered ? NUMBERED : BULLET;
      const items: ListItem[] = [];
      let start = 1;
      while (i < lines.length && re.test(lines[i])) {
        const m = lines[i++].match(re)!;
        if (items.length === 0 && ordered) start = parseInt(m[2], 10);
        const level = Math.min(3, Math.floor(m[1].replace(/\t/g, "    ").length / 2) + (m[2] === "◦" ? 1 : 0));
        items.push({ level, c: inline(m[3]) });
      }
      out.push({ t: "list", ordered, start, items });
      continue;
    }
    const para: Inline[][] = [];
    while (
      i < lines.length &&
      !QUOTE.test(lines[i]) &&
      !HEADING.test(lines[i]) &&
      !BULLET.test(lines[i]) &&
      !NUMBERED.test(lines[i])
    ) {
      para.push(inline(lines[i++]));
    }
    // Drop blank edges, keep blank lines inside as spacing.
    while (para.length && isBlank(para[0])) para.shift();
    while (para.length && isBlank(para[para.length - 1])) para.pop();
    if (para.length) out.push({ t: "p", lines: para });
  }
}

const isBlank = (l: Inline[]) => l.every((n) => n.t === "text" && n.v.trim() === "");

const BEFORE = /[\s([{"'«“\-/]/;
const AFTER = /[\s.,!?;:)\]}"'»”\-/]/;
const EMOJI = /^:([a-z0-9_+\-'.]+(?:::skin-tone-[2-6])?):/i;
const BARE_URL = /^https?:\/\/[^\s<>]+/;

export function inline(s: string): Inline[] {
  const out: Inline[] = [];
  let buf = "";
  const flush = () => {
    if (buf) out.push({ t: "text", v: unescape(buf) });
    buf = "";
  };
  let i = 0;
  while (i < s.length) {
    const ch = s[i];
    const boundaryBefore = i === 0 || BEFORE.test(s[i - 1]);

    if (ch === "<") {
      const j = s.indexOf(">", i + 1);
      if (j > i + 1) {
        flush();
        out.push(angle(s.slice(i + 1, j)));
        i = j + 1;
        continue;
      }
    } else if (ch === "`") {
      const j = s.indexOf("`", i + 1);
      if (j > i + 1) {
        flush();
        out.push({ t: "code", v: unescape(s.slice(i + 1, j)) });
        i = j + 1;
        continue;
      }
    } else if (ch === ":") {
      const m = s.slice(i).match(EMOJI);
      if (m) {
        flush();
        out.push({ t: "emoji", name: m[1] });
        i += m[0].length;
        continue;
      }
    } else if ((ch === "*" || ch === "_" || ch === "~") && boundaryBefore && s[i + 1] && s[i + 1] !== " ") {
      const k = closer(s, i, ch);
      if (k > 0) {
        flush();
        out.push({ t: ch === "*" ? "b" : ch === "_" ? "i" : "s", c: inline(s.slice(i + 1, k)) });
        i = k + 1;
        continue;
      }
    } else if (ch === "h" && boundaryBefore) {
      const m = s.slice(i).match(BARE_URL);
      if (m) {
        const url = trimUrl(m[0]);
        flush();
        out.push({ t: "link", url: unescape(url) });
        i += url.length;
        continue;
      }
    }
    buf += ch;
    i++;
  }
  flush();
  return out;
}

/// Index of the marker closing the one at `open`, or -1.
function closer(s: string, open: number, ch: string): number {
  for (let k = open + 2; k < s.length; k++) {
    if (s[k] === "\n") return -1;
    if (s[k] === ch && s[k - 1] !== " " && (k + 1 === s.length || AFTER.test(s[k + 1]))) return k;
  }
  return -1;
}

function angle(token: string): Inline {
  const bar = token.indexOf("|");
  const target = bar >= 0 ? token.slice(0, bar) : token;
  const label = bar >= 0 ? unescape(token.slice(bar + 1)) : undefined;
  switch (target[0]) {
    case "@":
      return { t: "user", id: target.slice(1), label };
    case "#":
      return { t: "channel", id: target.slice(1), label };
    case "!": {
      const name = target.slice(1);
      if (["here", "channel", "everyone"].includes(name)) return { t: "special", v: "@" + name };
      // subteam^S…|@team, date^…|fallback and the like carry their own label.
      return { t: "special", v: label ?? "@" + name.split("^")[0] };
    }
    default:
      return { t: "link", url: unescape(target), label: label ? inline(label) : undefined };
  }
}
