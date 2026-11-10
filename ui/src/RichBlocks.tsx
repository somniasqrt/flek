import { For, Show, type JSX } from "solid-js";
import { Link } from "./Rich";
import Emoji from "./EmojiView";
import { people } from "./people";
import { safeLink } from "./rich-compose";

// A deliberately small read-only renderer for the composer's documented rich text.

type Node = {
  type?: string;
  text?: string;
  url?: string;
  name?: string;
  skin_tone?: number;
  user_id?: string;
  style?: Record<string, unknown> | string;
  indent?: number;
  elements?: unknown[];
};
const node = (v: unknown): Node => (v && typeof v === "object" ? (v as Node) : {});
const children = (v: Node) => (Array.isArray(v.elements) ? v.elements : []);
const text = (v: unknown) => (typeof v === "string" ? v : "");
export const hasRichBlocks = (raw: unknown) => {
  const supported = (raw: unknown, depth = 0): boolean => {
    if (depth > 4) return false;
    const n = node(raw);
    if (["text", "link", "user", "emoji"].includes(n.type ?? "")) return true;
    return (
      [
        "rich_text",
        "rich_text_section",
        "rich_text_list",
        "rich_text_preformatted",
        "rich_text_quote",
      ].includes(n.type ?? "") &&
      children(n).length > 0 &&
      children(n).every((v) => supported(v, depth + 1))
    );
  };
  return (
    Array.isArray(raw) &&
    raw.length > 0 &&
    raw.every((v) => node(v).type === "rich_text" && supported(v))
  );
};

function inline(raw: unknown): JSX.Element {
  const n = node(raw);
  let content: JSX.Element;
  if (n.type === "user")
    content = <span class="mention">@{people()[text(n.user_id)] ?? text(n.user_id)}</span>;
  else if (n.type === "emoji") {
    const name = text(n.name);
    const tone = n.skin_tone;
    content = (
      <Emoji
        name={
          typeof tone === "number" &&
          Number.isInteger(tone) &&
          tone >= 2 &&
          tone <= 6 &&
          !name.includes("::skin-tone-")
            ? `${name}::skin-tone-${tone}`
            : name
        }
        inText
      />
    );
  } else if (n.type === "link") {
    const url = text(n.url);
    const label = text(n.text);
    content = safeLink(url) ? (
      <Link url={url} label={label && label !== url ? [{ t: "text", v: label }] : undefined} />
    ) : (
      label || url
    );
  } else content = text(n.text);
  // Wrap from the inside out; never interpret stored text as HTML.
  const style = n.style && typeof n.style === "object" ? n.style : {};
  if (style.code) {
    const inner = content;
    content = <code>{inner}</code>;
  }
  if (style.bold) {
    const inner = content;
    content = <strong>{inner}</strong>;
  }
  if (style.italic) {
    const inner = content;
    content = <em>{inner}</em>;
  }
  if (style.underline) {
    const inner = content;
    content = <u>{inner}</u>;
  }
  if (style.strike) {
    const inner = content;
    content = <s>{inner}</s>;
  }
  return content;
}
function section(raw: unknown): JSX.Element {
  const n = node(raw);
  const content = () => <For each={children(n)}>{inline}</For>;
  if (n.type === "rich_text_list") {
    const indent = Math.min(3, Math.max(0, Number(n.indent) || 0));
    const items = () => (
      <For each={children(n)}>
        {(item) => (
          <li>
            <For each={children(node(item))}>{inline}</For>
          </li>
        )}
      </For>
    );
    return n.style === "ordered" ? (
      <ol style={{ "margin-left": `${indent * 18}px` }}>{items()}</ol>
    ) : (
      <ul style={{ "margin-left": `${indent * 18}px` }}>{items()}</ul>
    );
  }
  if (n.type === "rich_text_preformatted") return <pre>{content()}</pre>;
  if (n.type === "rich_text_quote") return <blockquote>{content()}</blockquote>;
  return <p>{content()}</p>;
}
export default function RichBlocks(props: { blocks: unknown }) {
  const blocks = () =>
    Array.isArray(props.blocks) ? props.blocks.filter((v) => node(v).type === "rich_text") : [];
  return (
    <div class="rich rich-blocks">
      <For each={blocks()}>
        {(b) => (
          <Show when={children(node(b))}>{(items) => <For each={items()}>{section}</For>}</Show>
        )}
      </For>
    </div>
  );
}
