import { For, Show, type JSX } from "solid-js";
import Rich, { Link } from "./Rich";
import { linkImageUrl } from "../api";
import {
  parseBlockKit,
  supportedRich,
  type TextObject,
  type BlockImage,
} from "../models/block-kit";
import Emoji from "./EmojiView";
import { settings } from "../stores/settings";
import { people } from "../stores/people";
import { safeLink } from "../models/rich-compose";

// Read-only received Block Kit and the composer's documented rich text.

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
export const hasRichBlocks = (raw: unknown) =>
  Array.isArray(raw) &&
  raw.length > 0 &&
  raw.every((v) => node(v).type === "rich_text" && supportedRich(v));

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
function Text(props: { value: TextObject }) {
  return props.value.type === "mrkdwn" ? (
    <Rich text={props.value.text} />
  ) : (
    <span class="block-plain">{props.value.text}</span>
  );
}
function Image(props: { value: BlockImage; images: Record<string, string> }) {
  const cached = () =>
    settings().image_previews &&
    Object.hasOwn(props.images, props.value.image_url) &&
    typeof props.images[props.value.image_url] === "string"
      ? props.images[props.value.image_url]
      : undefined;
  return (
    <Show when={cached()} fallback={<span class="muted">{props.value.alt_text}</span>}>
      {(hash) => (
        <img
          class="block-image"
          src={linkImageUrl(hash())}
          alt={props.value.alt_text}
          loading="lazy"
        />
      )}
    </Show>
  );
}
export default function RichBlocks(props: { blocks: unknown; images?: Record<string, string> }) {
  const blocks = () => parseBlockKit(props.blocks).blocks;
  return (
    <div class="rich rich-blocks">
      <For each={blocks()}>
        {(b) => {
          switch (b.type) {
            case "rich_text":
              return <For each={b.elements}>{section}</For>;
            case "divider":
              return <hr />;
            case "header":
              return (
                <h3>
                  <Text value={b.text} />
                </h3>
              );
            case "image":
              return (
                <figure>
                  <Show when={b.title}>
                    {(title) => (
                      <figcaption>
                        <Text value={title()} />
                      </figcaption>
                    )}
                  </Show>
                  <Image value={b} images={props.images ?? {}} />
                </figure>
              );
            case "section":
              return (
                <div class="block-section">
                  <div>
                    <Show when={b.text}>{(text) => <Text value={text()} />}</Show>
                    <div class="block-fields">
                      <For each={b.fields}>{(f) => <Text value={f} />}</For>
                    </div>
                  </div>
                  <Show when={b.accessory}>
                    {(img) => <Image value={img()} images={props.images ?? {}} />}
                  </Show>
                </div>
              );
            case "context":
              return (
                <div class="block-context">
                  <For each={b.elements}>
                    {(e) =>
                      e.type === "image" ? (
                        <Image value={e} images={props.images ?? {}} />
                      ) : (
                        <Text value={e} />
                      )
                    }
                  </For>
                </div>
              );
            case "actions":
              return (
                <div class="block-actions">
                  <For each={b.elements}>
                    {(e) => (
                      <button type="button" disabled>
                        <Text value={e.text} />
                      </button>
                    )}
                  </For>
                </div>
              );
          }
        }}
      </For>
    </div>
  );
}
