import createDOMPurify from "dompurify";
import { HTML_LIMIT } from "./document.ts";

export const rasterDataUri = (src: string) =>
  /^data:image\/(png|jpeg|gif|webp|avif|bmp);base64,[a-z0-9+/]*={0,2}$/i.test(
    src,
  );

/** No links, CSS, ids, active content, remote images or navigation attributes. */
export function sanitizeDocument(
  html: string,
  window: Window & typeof globalThis,
): string {
  if (html.length > HTML_LIMIT) throw new Error("document_limit");
  const purify = createDOMPurify(window);
  purify.addHook("uponSanitizeAttribute", (node, data) => {
    if (data.attrName === "src")
      data.keepAttr = node.nodeName === "IMG" && rasterDataUri(data.attrValue);
    if (data.attrName === "colspan" || data.attrName === "rowspan") {
      data.keepAttr = /^(?:[1-9]|[1-9][0-9]|100)$/.test(data.attrValue);
    }
  });
  const body = purify.sanitize(html, {
    ALLOWED_TAGS: [
      "p",
      "br",
      "h1",
      "h2",
      "h3",
      "h4",
      "h5",
      "h6",
      "strong",
      "b",
      "em",
      "i",
      "u",
      "s",
      "del",
      "sup",
      "sub",
      "ul",
      "ol",
      "li",
      "table",
      "thead",
      "tbody",
      "tfoot",
      "tr",
      "th",
      "td",
      "blockquote",
      "pre",
      "code",
      "hr",
      "img",
    ],
    ALLOWED_ATTR: ["src", "alt", "colspan", "rowspan"],
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
    RETURN_DOM: true,
  }) as HTMLElement;
  const nodes = body.querySelectorAll("*");
  if (nodes.length > 20000) throw new Error("document_limit");
  for (const node of nodes) {
    let depth = 0;
    for (let p = node.parentElement; p; p = p.parentElement)
      if (++depth > 64) throw new Error("document_limit");
    if (node.tagName === "IMG" && !node.hasAttribute("src")) node.remove();
  }
  return body.innerHTML;
}
