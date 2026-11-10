// A search snippet (raw Slack mrkdwn with match markers from core) → plain
// text pieces, matched terms flagged, for one line of a result.

export const MARK_OPEN = "\u0002";
export const MARK_CLOSE = "\u0003";

export type Piece = { text: string; mark: boolean };

const TOKEN = /<([^<>]*)>/g;
/// `*bold*`, `_italic_`, `~strike~` between word boundaries (not snake_case).
const EMPHASIS = /(^|[\s(\u0002])([*_~])(?=\S)([^*_~\n]*?\S)\2(?=$|[\s.,!?;:)\u0003])/g;
const EMOJI = /:([a-z0-9_+'-]+(?:::skin-tone-\d)?):/g;
const strip = (s: string) => s.replaceAll(MARK_OPEN, "").replaceAll(MARK_CLOSE, "");

/// Slack tokens (`<@U1>`, `<#C1|general>`, `<https://x|label>`) become what
/// the timeline would show. A token that contained a match is marked whole,
/// so a match inside a hidden URL still shows up. Emphasis marks go, emoji
/// names become characters when `emoji` knows them.
export function readable(
  snippet: string,
  people: Record<string, string> = {},
  emoji: (name: string) => string | undefined = () => undefined,
): string {
  const text = snippet.replace(TOKEN, (_token, inner: string) => {
    const [rawTarget, label] = splitOnce(inner, "|");
    const target = strip(rawTarget);
    let out: string;
    if (target.startsWith("@")) out = "@" + (label || people[target.slice(1)] || target.slice(1));
    else if (target.startsWith("#")) out = "#" + (label || target.slice(1));
    else if (target.startsWith("!")) out = "@" + (label || target.slice(1).split("^")[0]);
    else out = label || target.replace(/^mailto:/, "");
    // Markers in the label stay where they are; a match in the target (often
    // not shown) marks the whole token.
    return rawTarget.includes(MARK_OPEN) ? MARK_OPEN + strip(out) + MARK_CLOSE : out;
  });
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
    .replace(/```/g, "")
    .replace(EMPHASIS, "$1$3")
    .replace(EMOJI, (all, name: string) => emoji(name) ?? all)
    .replace(/\s+/g, " ")
    .trim();
}

/// Splits on the markers. Tolerates unbalanced ones (FTS5 cuts snippets
/// at token boundaries, but a cut can still fall between the two).
export function pieces(text: string): Piece[] {
  const out: Piece[] = [];
  let mark = false;
  let cur = "";
  const flush = () => {
    if (cur) out.push({ text: cur, mark });
    cur = "";
  };
  for (const c of text) {
    if (c === MARK_OPEN || c === MARK_CLOSE) {
      flush();
      mark = c === MARK_OPEN;
    } else {
      cur += c;
    }
  }
  flush();
  return out;
}

function splitOnce(s: string, sep: string): [string, string] {
  const i = s.indexOf(sep);
  return i < 0 ? [s, ""] : [s.slice(0, i), s.slice(i + 1)];
}
