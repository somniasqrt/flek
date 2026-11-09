import catalog from "./emoji-catalog.json";
export type PickerEmoji = {
  name: string;
  label: string;
  group: string;
  terms: string;
  tones?: string[];
};
export const emojiCatalog: PickerEmoji[] = catalog;
export const emojiByName = new Map(emojiCatalog.map((e) => [e.name, e]));
export const categories = [
  { id: "people", label: "Smileys & people" },
  { id: "nature", label: "Animals & nature" },
  { id: "food", label: "Food & drink" },
  { id: "travel", label: "Travel & places" },
  { id: "activities", label: "Activities" },
  { id: "objects", label: "Objects" },
  { id: "symbols", label: "Symbols" },
  { id: "flags", label: "Flags" },
  { id: "custom", label: "Workspace emoji" },
];
export const frequentDefaults = [
  "heart",
  "white_check_mark",
  "cherry_blossom",
  "fire",
  "saluting_face",
  "thinking_face",
  "+1",
  "hearts",
  "butterfly",
  "balloon",
  "chipmunk",
  "christmas_tree",
  "crown",
  "cry",
  "dizzy",
  "gift",
  "heart_on_fire",
  "hibiscus",
  "innocent",
  "rabbit",
  "raised_hands",
  "ringed_planet",
  "santa",
  "skull",
  "slightly_smiling_face",
  "smiling_imp",
  "snowflake",
  "snowman",
  "space_invader",
  "sparkling_heart",
  "star-struck",
  "sunny",
  "tada",
  "trophy",
];
export const workEmoji = [
  "white_check_mark",
  "eyes",
  "raised_hands",
  "pray",
  "heavy_plus_sign",
  "clap",
  "bulb",
  "dart",
  "wave",
  "+1",
  "tada",
  "one",
  "two",
  "three",
  "mega",
  "white_circle",
  "large_blue_circle",
  "red_circle",
];
const FREQUENT_KEY = "flek.emoji.frequency";
export function readFrequency(): Record<string, number> {
  try {
    const value = JSON.parse(localStorage.getItem(FREQUENT_KEY) ?? "{}");
    return Object.fromEntries(
      Object.entries(value ?? {}).filter(
        ([name, n]) =>
          /^[a-z0-9_+:.\-']+$/i.test(name) && typeof n === "number" && Number.isFinite(n) && n > 0,
      ),
    ) as Record<string, number>;
  } catch {
    return {};
  }
}
export function rememberEmoji(name: string) {
  try {
    const frequency = readFrequency();
    frequency[name] = (frequency[name] ?? 0) + 1;
    const top = Object.entries(frequency)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 80);
    localStorage.setItem(FREQUENT_KEY, JSON.stringify(Object.fromEntries(top)));
  } catch {
    /* optional preference */
  }
}
export function readSkinTone() {
  try {
    const n = Number(localStorage.getItem("flek.emoji.skinTone"));
    return Number.isInteger(n) && n >= 2 && n <= 6 ? n : 0;
  } catch {
    return 0;
  }
}
export const tonedName = (name: string, tone: number) =>
  tone && !name.includes("::") && emojiByName.get(name)?.tones
    ? `${name}::skin-tone-${tone}`
    : name;
