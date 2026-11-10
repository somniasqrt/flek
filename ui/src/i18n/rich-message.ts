import { interpolate, type Params } from "./index.ts";

type Part = { text: string } | { tag: string; children: Part[] };

// Parse dictionary markup before inserting parameters. Only paired, attribute-free
// tags are structural; malformed markup and unknown tags remain visible text.
function parse(template: string): Part[] {
  const root: Part[] = [];
  const stack = [{ tag: "", children: root }];
  const tokens = /<\/?([a-z][a-z0-9]*)>/g;
  let start = 0;
  for (const token of template.matchAll(tokens)) {
    const frame = stack.at(-1)!;
    if (token.index > start) frame.children.push({ text: template.slice(start, token.index) });
    if (token[0].startsWith("</")) {
      if (stack.length === 1 || frame.tag !== token[1]) return [{ text: template }];
      stack.pop();
    } else {
      const part = { tag: token[1], children: [] as Part[] };
      frame.children.push(part);
      stack.push(part);
    }
    start = token.index + token[0].length;
  }
  if (stack.length !== 1) return [{ text: template }];
  if (start < template.length) root.push({ text: template.slice(start) });
  return root;
}

export function renderRichMessage<T>(
  template: string,
  params: Params = {},
  tags: Record<string, (children: Array<string | T>) => T> = {},
): Array<string | T> {
  const render = (parts: Part[]): Array<string | T> =>
    parts.flatMap((part) => {
      if ("text" in part) return [interpolate(part.text, params)];
      const children = render(part.children);
      if (Object.hasOwn(tags, part.tag)) return [tags[part.tag](children)];
      return [`<${part.tag}>`, ...children, `</${part.tag}>`];
    });
  return render(parse(template));
}
