// Slack's emoji names → Unicode. Slack's set is iamcal's emoji-data, which
// emojibase ships as shortcodes (hexcode → name or names).
import iamcal from "emojibase-data/en/shortcodes/iamcal.json";

const byName = new Map<string, string>();
for (const [hex, names] of Object.entries(iamcal as Record<string, string | string[]>)) {
  const points = hex.split("-").map((h) => parseInt(h, 16));
  let char = String.fromCodePoint(...points);
  // Single old symbols (❤, ☺) need the emoji presentation selector.
  if (points.length === 1 && points[0] < 0x1f000) char += "️";
  for (const n of Array.isArray(names) ? names : [names]) byName.set(n, char);
}

const SKIN_TONES: Record<string, string> = {
  "skin-tone-2": "\u{1F3FB}",
  "skin-tone-3": "\u{1F3FC}",
  "skin-tone-4": "\u{1F3FD}",
  "skin-tone-5": "\u{1F3FE}",
  "skin-tone-6": "\u{1F3FF}",
};

/// `thumbsup`, `+1::skin-tone-3` → the character; `undefined` for custom
/// workspace emoji (shown as `:name:` until emoji.list lands, M3.4.2).
export function emoji(name: string): string | undefined {
  const [base, tone] = name.split("::");
  const char = byName.get(base);
  if (!char) return undefined;
  const mod = tone ? SKIN_TONES[tone] : undefined;
  return mod ? char.replace("️", "") + mod : char;
}
