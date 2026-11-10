import { For, Show } from "solid-js";
import { api, linkImageUrl, type Unfurl } from "./api";
import { ParsedRich } from "./Rich";
import { parse } from "./mrkdwn";
import { previewBlocks, previewText } from "./unfurl-text";

// Link previews under a message, laid out like Slack's: a bar on the left,
// the site, a linked title, the description and the picture.

const MAX_W = 360;
const MAX_H = 240;

export default function Unfurls(props: { items: Unfurl[] }) {
  return <For each={props.items}>{(u) => <Card u={u} />}</For>;
}

function open(url: string | null, e?: MouseEvent) {
  e?.preventDefault();
  if (url) api.openUrl(url).catch(() => {});
}

function Card(props: { u: Unfurl }) {
  const u = props.u;
  const link = u.title_link ?? u.url;
  const size = () => {
    const w = u.image_width ?? MAX_W;
    const h = u.image_height ?? MAX_H;
    const k = Math.min(1, MAX_W / w, MAX_H / h);
    return { width: `${Math.round(w * k)}px`, height: `${Math.round(h * k)}px` };
  };
  return (
    <div class="unfurl" style={u.color ? { "--bar": u.color } : {}}>
      <div class="unfurl-body">
        <Show when={u.service || u.author}>
          <div class="unfurl-site">
            <Show when={u.icon}>
              {(i) => <img class="unfurl-icon" src={linkImageUrl(i())} alt="" />}
            </Show>
            <span>{previewText(u.service ?? u.author)}</span>
          </div>
        </Show>
        <Show when={previewText(u.title)}>
          <a class="unfurl-title" href={link ?? undefined} onClick={(e) => open(link, e)}>
            {previewText(u.title)}
          </a>
        </Show>
        <Show when={u.text}>{(t) => <ParsedRich blocks={previewBlocks(parse(t()))} />}</Show>
        <Show when={u.fields.length > 0}>
          <div class="unfurl-fields">
            <For each={u.fields}>
              {([k, v]) => (
                <div>
                  <div class="unfurl-field-name">{previewText(k)}</div>
                  <ParsedRich blocks={previewBlocks(parse(v))} />
                </div>
              )}
            </For>
          </div>
        </Show>
        <Show when={u.image && !u.thumb}>
          <button class="unfurl-image" style={size()} onClick={() => open(link)}>
            <img src={linkImageUrl(u.image!)} alt="" loading="lazy" />
            <Show when={u.video}>
              <span class="play" aria-hidden="true">
                <svg viewBox="0 0 20 20">
                  <path d="M7 5.5v9l7.5-4.5z" />
                </svg>
              </span>
            </Show>
          </button>
        </Show>
        <Show when={u.footer}>
          <div class="unfurl-footer muted">{previewText(u.footer)}</div>
        </Show>
      </div>
      <Show when={u.image && u.thumb}>
        <img class="unfurl-thumb" src={linkImageUrl(u.image!)} alt="" loading="lazy" />
      </Show>
    </div>
  );
}
