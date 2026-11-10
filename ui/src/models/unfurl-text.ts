// Website preview metadata may contain HTML entities beyond Slack's &amp;/&lt;/&gt;.
import { decodeHTMLStrict } from "entities/decode";
import type { Block, Inline } from "./mrkdwn";

export const previewText = (text: string | null) =>
  decodeHTMLStrict(text ?? "");

// Decode after mrkdwn parsing: an encoded '<@U1>' stays literal text, not a
// mention or a link. Actual links and their URLs retain Slack's own parsing.
function inlines(list: Inline[]): Inline[] {
  return list.map((n) => {
    switch (n.t) {
      case "text":
      case "code":
      case "special":
        return { ...n, v: previewText(n.v) };
      case "b":
      case "i":
      case "s":
        return { ...n, c: inlines(n.c) };
      case "link":
        return { ...n, label: n.label ? inlines(n.label) : undefined };
      case "user":
      case "channel":
        return { ...n, label: n.label ? previewText(n.label) : undefined };
      default:
        return n;
    }
  });
}

export function previewBlocks(list: Block[]): Block[] {
  return list.map((b) => {
    switch (b.t) {
      case "p":
        return { ...b, lines: b.lines.map(inlines) };
      case "h":
        return { ...b, c: inlines(b.c) };
      case "quote":
        return { ...b, c: previewBlocks(b.c) };
      case "list":
        return { ...b, items: b.items.map((item) => ({ ...item, c: inlines(item.c) })) };
      case "pre":
        return { ...b, v: previewText(b.v) };
    }
  });
}
