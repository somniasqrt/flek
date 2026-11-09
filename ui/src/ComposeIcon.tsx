import { Show } from "solid-js";
export type ComposeIconName =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "paragraph"
  | "chevron"
  | "link"
  | "ordered"
  | "bullet"
  | "quote"
  | "code"
  | "pre"
  | "clear"
  | "plus"
  | "format"
  | "emoji"
  | "mention"
  | "video"
  | "mic"
  | "canvas"
  | "send";
const paths: Record<
  Exclude<ComposeIconName, "bold" | "italic" | "underline" | "strike" | "format">,
  string
> = {
  paragraph: "M12 20V4H9a4 4 0 0 0 0 8h3M17 4v16M9 4h12",
  chevron: "m7 9 5 5 5-5",
  link: "m9 15 6-6M8 16l-1 1a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0M16 8l1-1a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0",
  ordered: "M10 5h11M10 12h11M10 19h11M3 3h1v5M2 8h4M2 14c0-3 4-3 4 0 0 1-4 2-4 5h4",
  bullet: "M10 5h11M10 12h11M10 19h11M3 5h1M3 12h1M3 19h1",
  quote: "M4 3v18M9 5h12M9 11h9M9 17h12",
  code: "m8 7-5 5 5 5m8-10 5 5-5 5M14 4l-4 16",
  pre: "M3 6v14h18V6M7 3 4 6l3 3m10-6 3 3-3 3M13 2l-2 8",
  clear: "M5 7h14M8 3h8M8 21h8M5 17h14M5 11h14M5 13h14",
  plus: "M12 4v16M4 12h16",
  emoji: "M20 12a8 8 0 1 1-16 0 8 8 0 0 1 16 0M8 14c2 3 6 3 8 0M8 9h.01M16 9h.01",
  mention: "M16 12a4 4 0 1 1-4-4 4 4 0 0 1 4 4Zm0 0V8m0 4v2c0 3 6 3 6-2a10 10 0 1 0-4 8",
  video: "M3 6h11v12H3zM14 10l7-4v12l-7-4",
  mic: "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0ZM5 11v1a7 7 0 0 0 14 0v-1M12 19v3M9 22h6",
  canvas: "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2M8 17l8-10",
  send: "m4 4 17 8-17 8 3-8-3-8Zm3 8h14",
};
const glyphs: Partial<Record<ComposeIconName, string>> = {
  bold: "B",
  italic: "I",
  underline: "U",
  strike: "S",
  format: "Aa",
};
export default function ComposeIcon(props: { name: ComposeIconName }) {
  const text = () => glyphs[props.name];
  return (
    <Show
      when={text()}
      fallback={
        <svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
          <path d={paths[props.name as keyof typeof paths]} />
        </svg>
      }
    >
      <span class={`compose-glyph compose-glyph-${props.name}`} aria-hidden="true">
        {text()}
      </span>
    </Show>
  );
}
