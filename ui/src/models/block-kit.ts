export type TextObject = { type: "plain_text" | "mrkdwn"; text: string };
export type BlockImage = { type: "image"; image_url: string; alt_text: string };
export type KitBlock =
  | { type: "rich_text"; elements: unknown[] }
  | { type: "section"; text?: TextObject; fields: TextObject[]; accessory?: BlockImage }
  | { type: "context"; elements: (TextObject | BlockImage)[] }
  | (BlockImage & { title?: TextObject })
  | { type: "divider" }
  | { type: "header"; text: TextObject }
  | { type: "actions"; elements: { text: TextObject }[] };
export const record = (v: unknown): Record<string, unknown> =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
function textObject(v: unknown): TextObject | undefined {
  const n = record(v);
  if ((n.type === "plain_text" || n.type === "mrkdwn") && typeof n.text === "string")
    return { type: n.type, text: n.text };
}
function image(v: unknown): BlockImage | undefined {
  const n = record(v);
  if (n.type === "image" && typeof n.image_url === "string" && typeof n.alt_text === "string")
    return { type: "image", image_url: n.image_url, alt_text: n.alt_text };
}
export function supportedRich(raw: unknown, depth = 0): boolean {
  if (depth > 4) return false;
  const n = record(raw);
  if (["text", "link", "user", "emoji"].includes(String(n.type))) return true;
  if (n.type === "channel") return typeof n.channel_id === "string" && !!n.channel_id;
  if (n.type === "broadcast") return ["here", "channel", "everyone"].includes(String(n.range));
  return (
    [
      "rich_text",
      "rich_text_section",
      "rich_text_list",
      "rich_text_preformatted",
      "rich_text_quote",
    ].includes(String(n.type)) &&
    Array.isArray(n.elements) &&
    n.elements.length > 0 &&
    n.elements.every((v) => supportedRich(v, depth + 1))
  );
}
function block(v: unknown): KitBlock | undefined {
  const n = record(v);
  if (n.type === "rich_text" && Array.isArray(n.elements) && supportedRich(n))
    return { type: "rich_text", elements: n.elements };
  if (n.type === "divider") return { type: "divider" };
  if (n.type === "header") {
    const text = textObject(n.text);
    if (text) return { type: "header", text };
  }
  if (n.type === "image") {
    const img = image(n);
    const title = n.title === undefined ? undefined : textObject(n.title);
    if (img && (n.title === undefined || title)) return { ...img, title };
  }
  if (n.type === "section") {
    const text = textObject(n.text);
    const fields =
      n.fields === undefined
        ? []
        : Array.isArray(n.fields)
          ? n.fields.map(textObject)
          : [undefined];
    const accessory = image(n.accessory);
    if (
      (!text && !fields.length) ||
      (n.text !== undefined && !text) ||
      fields.some((f) => !f) ||
      (n.accessory !== undefined && !accessory)
    )
      return;
    return { type: "section", text, fields: fields as TextObject[], accessory };
  }
  if (n.type === "context" && Array.isArray(n.elements) && n.elements.length) {
    const elements = n.elements.map((v) => textObject(v) ?? image(v));
    if (elements.every(Boolean))
      return { type: "context", elements: elements as (TextObject | BlockImage)[] };
  }
  if (n.type === "actions" && Array.isArray(n.elements) && n.elements.length) {
    const elements = n.elements.map((v) => {
      const e = record(v);
      return e.type === "button" ? textObject(e.text) : undefined;
    });
    if (elements.every(Boolean))
      return { type: "actions", elements: (elements as TextObject[]).map((text) => ({ text })) };
  }
}
/** All-or-fallback avoids duplicating or silently dropping unknown content. */
export function parseBlockKit(raw: unknown): { blocks: KitBlock[]; fallback: boolean } {
  if (raw === undefined || raw === null || (Array.isArray(raw) && !raw.length))
    return { blocks: [], fallback: false };
  if (!Array.isArray(raw) || raw.length > 100) return { blocks: [], fallback: true };
  const parsed = raw.map(block);
  return parsed.every(Boolean)
    ? { blocks: parsed as KitBlock[], fallback: false }
    : { blocks: [], fallback: true };
}
