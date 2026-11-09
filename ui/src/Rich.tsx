// Renders a message's mrkdwn text: blocks, inline styles, mentions, emoji
// and links (meeting / doc / video links get a service badge).
import { For, Match, Show, Switch, type JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { api } from "./api";
import RichBlocks, { hasRichBlocks } from "./RichBlocks";
import Emoji from "./EmojiView";
import { linkInfo, shortUrl, type LinkKind } from "./links";
import { parse, type Block, type Inline } from "./mrkdwn";
import { people } from "./people";

export default function Rich(props: { text: string; blocks?: unknown }) {
  return <Show when={hasRichBlocks(props.blocks)} fallback={<div class="rich">{blocks(parse(props.text))}</div>}><RichBlocks blocks={props.blocks} /></Show>;
}

/// Render an already parsed tree without interpreting decoded website text as HTML.
export function ParsedRich(props: { blocks: Block[] }) {
  return <div class="rich">{blocks(props.blocks)}</div>;
}

function blocks(list: Block[]): JSX.Element {
  return (
    <For each={list}>
      {(b) => (
        <Switch>
          <Match when={b.t === "p" && b}>
            {(p) => (
              <p>
                <For each={p().lines}>
                  {(line, i) => (
                    <>
                      <Show when={i() > 0}>
                        <br />
                      </Show>
                      {inlines(line)}
                    </>
                  )}
                </For>
              </p>
            )}
          </Match>
          <Match when={b.t === "h" && b}>
            {(h) => <div class={`h h${h().level}`}>{inlines(h().c)}</div>}
          </Match>
          <Match when={b.t === "quote" && b}>{(q) => <blockquote>{blocks(q().c)}</blockquote>}</Match>
          <Match when={b.t === "pre" && b}>{(c) => <pre>{c().v}</pre>}</Match>
          <Match when={b.t === "list" && b}>
            {(l) => {
              return (
                <Dynamic
                  component={l().ordered ? "ol" : "ul"}
                  class="list"
                  start={l().ordered ? l().start : undefined}
                >
                  <For each={l().items}>
                    {(item) => <li style={{ "margin-left": `${item.level * 18}px` }}>{inlines(item.c)}</li>}
                  </For>
                </Dynamic>
              );
            }}
          </Match>
        </Switch>
      )}
    </For>
  );
}

function inlines(list: Inline[]): JSX.Element {
  return <For each={list}>{(n) => node(n)}</For>;
}

function node(n: Inline): JSX.Element {
  switch (n.t) {
    case "text":
      return n.v;
    case "b":
      return <strong>{inlines(n.c)}</strong>;
    case "i":
      return <em>{inlines(n.c)}</em>;
    case "s":
      return <s>{inlines(n.c)}</s>;
    case "code":
      return <code>{n.v}</code>;
    case "user":
      return <span class="mention">@{people()[n.id] ?? n.label ?? n.id}</span>;
    case "channel":
      return <span class="mention">#{n.label ?? n.id}</span>;
    case "special":
      return <span class="mention special">{n.v}</span>;
    case "emoji":
      return <Emoji name={n.name} inText />;
    case "link":
      return <Link url={n.url} label={n.label} />;
  }
}

export function Link(props: { url: string; label?: Inline[] }) {
  const info = linkInfo(props.url);
  const open = (e: MouseEvent) => {
    // Never navigate Flek's own window; links go to the browser.
    e.preventDefault();
    api.openUrl(props.url).catch(() => {});
  };
  if (props.url.startsWith("mailto:")) {
    return (
      <a class="link" href={props.url} onClick={open}>
        {props.label ? inlines(props.label) : props.url.slice(7)}
      </a>
    );
  }
  // A known service, or a bare URL: a chip with an icon. Labelled links to
  // ordinary sites stay inline text.
  if (info.kind !== "web" || !props.label) {
    return (
      <a
        class="chip"
        href={props.url}
        onClick={open}
        title={props.url}
        style={{ "--chip": info.color }}
      >
        <LinkIcon kind={info.kind} />
        <Show when={info.kind !== "web"}>
          <span class="service">{info.service}</span>
        </Show>
        <span class="label">{props.label ? inlines(props.label) : shortUrl(props.url)}</span>
      </a>
    );
  }
  return (
    <a class="link" href={props.url} onClick={open} title={props.url}>
      {inlines(props.label)}
    </a>
  );
}

/// Small line icons, drawn here (no third-party logos).
export function LinkIcon(props: { kind: LinkKind }) {
  const paths: Record<LinkKind, string> = {
    meeting: "M3 6.5h9a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5H3A1.5 1.5 0 0 1 1.5 16V8A1.5 1.5 0 0 1 3 6.5Zm10.5 4 5-3v9l-5-3",
    doc: "M5 2.5h7l4 4V17a.5.5 0 0 1-.5.5h-10A.5.5 0 0 1 5 17V3a.5.5 0 0 1 .5-.5ZM12 2.5V7h4M7.5 10h6M7.5 13h6",
    video: "M2.5 5.5h15v9h-15zM8.5 7.8v4.4l3.8-2.2z",
    code: "M7 6 3 10l4 4M13 6l4 4-4 4M11 4.5l-2 11",
    web: "M8.5 11.5a3 3 0 0 0 4.2 0l3-3a3 3 0 0 0-4.2-4.2l-1 1M11.5 8.5a3 3 0 0 0-4.2 0l-3 3a3 3 0 0 0 4.2 4.2l1-1",
  };
  return (
    <svg class="icon" viewBox="0 0 20 20" aria-hidden="true">
      <path d={paths[props.kind]} />
    </svg>
  );
}
