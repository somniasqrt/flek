// The editor DOM never leaves the UI. Send a small documented Slack rich_text tree.
export type TextStyle = Partial<
  Record<"bold" | "italic" | "underline" | "strike" | "code", boolean>
>;
export type RichInline =
  | { type: "text"; text: string; style?: TextStyle }
  | { type: "link"; url: string; text?: string; style?: TextStyle }
  | { type: "user"; user_id: string }
  | { type: "emoji"; name: string };
export type RichSection = {
  type: "rich_text_section" | "rich_text_quote" | "rich_text_preformatted";
  elements: RichInline[];
};
export type RichList = {
  type: "rich_text_list";
  style: "ordered" | "bullet";
  indent: number;
  elements: RichSection[];
};
export type RichBlock = { type: "rich_text"; elements: (RichSection | RichList)[] };

export const safeLink = (url: string) => {
  try {
    return ["http:", "https:", "mailto:"].includes(new URL(url).protocol);
  } catch {
    return false;
  }
};

// Split URLs without a DOM; keep punctuation in the surrounding text.
export function linkify(text: string, style: TextStyle = {}): RichInline[] {
  const styled = Object.keys(style).length ? { style } : {};
  const plain = (text: string): RichInline => ({ type: "text", text, ...styled });
  if (style.code) return text ? [plain(text)] : [];
  const out: RichInline[] = [];
  let cursor = 0;
  for (const match of text.matchAll(/https?:\/\/[^\s<>]+/gi)) {
    let url = match[0];
    while (/[.,;:!?) ]$/.test(url)) {
      if (url.endsWith(")") && [...url].filter(c => c === "(").length >= [...url].filter(c => c === ")").length) break;
      url = url.slice(0, -1);
    }
    if (!safeLink(url)) continue;
    if (match.index! > cursor) out.push(plain(text.slice(cursor, match.index)));
    out.push({ type: "link", url, ...styled });
    cursor = match.index! + url.length;
  }
  if (cursor < text.length) out.push(plain(text.slice(cursor)));
  return out;
}

function inline(nodes: Iterable<Node>, style: TextStyle = {}): RichInline[] {
  const out: RichInline[] = [];
  for (const node of nodes) {
    if (node.nodeType === 3) {
      const text = (node.textContent ?? "").replace(/\u200b/g, "").replace(/\u00a0/g, " ");
      if (text) out.push(...linkify(text, style));
      continue;
    }
    if (!(node instanceof HTMLElement)) continue;
    const tag = node.tagName;
    if (["SCRIPT", "STYLE", "IFRAME", "IMG"].includes(tag)) continue;
    if (tag === "BR") {
      out.push({ type: "text", text: "\n" });
      continue;
    }
    if (node.dataset.emojiName) {
      out.push({ type: "emoji", name: node.dataset.emojiName });
      continue;
    }
    if (node.dataset.userId) {
      out.push({ type: "user", user_id: node.dataset.userId });
      continue;
    }
    const s = { ...style };
    if (
      ["B", "STRONG"].includes(tag) ||
      Number(node.style.fontWeight) >= 600 ||
      node.style.fontWeight === "bold"
    )
      s.bold = true;
    if (["I", "EM"].includes(tag) || node.style.fontStyle === "italic") s.italic = true;
    if (tag === "U" || node.style.textDecoration.includes("underline")) s.underline = true;
    if (["S", "STRIKE", "DEL"].includes(tag) || node.style.textDecoration.includes("line-through"))
      s.strike = true;
    if (tag === "CODE") s.code = true;
    if (tag === "A" && safeLink(node.getAttribute("href") ?? "")) {
      out.push({
        type: "link",
        url: node.getAttribute("href")!,
        text: node.textContent ?? "",
        ...(Object.keys(s).length ? { style: s } : {}),
      });
    } else {
      if (["DIV", "P"].includes(tag) && out.length) out.push({ type: "text", text: "\n" });
      out.push(...inline(node.childNodes, s));
    }
  }
  return out;
}

export function editorBlocks(editor: HTMLElement): RichBlock[] {
  const elements: (RichSection | RichList)[] = [];
  let run: Node[] = [];
  const flush = () => {
    const content = inline(run);
    run = [];
    if (content.length) elements.push({ type: "rich_text_section", elements: content });
  };
  const walk = (nodes: Iterable<Node>, indent = 0) => {
    for (const node of nodes) {
      if (node instanceof HTMLElement) {
        const tag = node.tagName;
        if (tag === "UL" || tag === "OL") {
          flush();
          for (const child of node.children) {
            if (child.tagName !== "LI") continue;
            const content = inline(
              [...child.childNodes].filter(
                (n) => !(n instanceof HTMLElement && ["UL", "OL"].includes(n.tagName)),
              ),
            );
            const style = tag === "OL" ? "ordered" : "bullet";
            const item: RichSection = {
              type: "rich_text_section",
              elements: content.length ? content : [{ type: "text", text: " " }],
            };
            const previous = elements[elements.length - 1];
            if (
              previous?.type === "rich_text_list" &&
              previous.style === style &&
              previous.indent === indent
            )
              previous.elements.push(item);
            else elements.push({ type: "rich_text_list", style, indent, elements: [item] });
            for (const nested of child.children)
              if (["UL", "OL"].includes(nested.tagName)) walk([nested], Math.min(indent + 1, 3));
          }
          continue;
        }
        if (tag === "PRE" || tag === "BLOCKQUOTE") {
          flush();
          const content =
            tag === "PRE"
              ? [{ type: "text" as const, text: node.innerText.replace(/\u200b/g, "") }]
              : inline(node.childNodes);
          if (content.length)
            elements.push({
              type: tag === "PRE" ? "rich_text_preformatted" : "rich_text_quote",
              elements: content,
            });
          continue;
        }
        if (tag === "DIV" || tag === "P") {
          flush();
          walk(node.childNodes, indent);
          flush();
          continue;
        }
      }
      run.push(node);
    }
  };
  walk(editor.childNodes);
  flush();
  return elements.length ? [{ type: "rich_text", elements }] : [];
}

// Plain fallback for notifications and FTS. Styling lives in blocks.
export function blocksText(blocks: RichBlock[]): string {
  return blocks
    .flatMap((b) =>
      b.elements.map((e) => {
        const section = (s: RichSection) =>
          s.elements
            .map((n) =>
              n.type === "text"
                ? n.text
                : n.type === "link"
                  ? n.text || n.url
                  : n.type === "emoji"
                    ? `:${n.name}:`
                    : `@${n.user_id}`,
            )
            .join("");
        return e.type === "rich_text_list" ? e.elements.map(section).join("\n") : section(e);
      }),
    )
    .join("\n")
    .trim();
}

// Drafts are untrusted local data. Rebuild only supported tags and attributes.
export function restoreEditor(editor: HTMLElement, draft: string) {
  editor.replaceChildren();
  let html: string | undefined;
  try {
    const saved = JSON.parse(draft);
    if (saved?.version === 1 && typeof saved.html === "string") html = saved.html;
  } catch {
    /* legacy plain-text draft */
  }
  if (html === undefined) {
    editor.textContent = draft;
    return;
  }
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const copy = (node: Node): Node => {
    if (node.nodeType === 3) return document.createTextNode(node.textContent ?? "");
    if (!(node instanceof HTMLElement) || ["SCRIPT", "STYLE", "IFRAME"].includes(node.tagName))
      return document.createTextNode("");
    const allowed = [
      "B",
      "STRONG",
      "I",
      "EM",
      "U",
      "S",
      "STRIKE",
      "CODE",
      "PRE",
      "BLOCKQUOTE",
      "P",
      "DIV",
      "BR",
      "UL",
      "OL",
      "LI",
      "A",
      "SPAN",
    ];
    const out = document.createElement(
      allowed.includes(node.tagName) ? node.tagName.toLowerCase() : "span",
    );
    if (node.tagName === "A" && safeLink(node.getAttribute("href") ?? ""))
      out.setAttribute("href", node.getAttribute("href")!);
    if (node.dataset.userId && /^[A-Z0-9]+$/.test(node.dataset.userId)) {
      out.dataset.userId = node.dataset.userId;
      out.contentEditable = "false";
      out.className = "mention";
    }
    if (
      node.dataset.emojiName &&
      /^[a-z0-9_+\-'.]+(?:::skin-tone-[2-6])?$/i.test(node.dataset.emojiName)
    ) {
      out.dataset.emojiName = node.dataset.emojiName;
      out.contentEditable = "false";
    }
    // WebKit uses spans for text decorations and font styling.
    for (const key of ["fontWeight", "fontStyle", "textDecoration", "fontFamily"] as const)
      out.style[key] = node.style[key];
    for (const child of node.childNodes) out.append(copy(child));
    return out;
  };
  for (const node of parsed.body.childNodes) editor.append(copy(node));
}
