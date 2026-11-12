import { isMac, keyCombo, modKeyLabel, shiftKeyLabel, type Platform } from "../platform.ts";

// Physical key codes keep shortcuts working with Ukrainian and other layouts.
export type ShortcutEvent = Pick<
  KeyboardEvent,
  "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing"
>;
export const usesMacShortcuts = () => isMac();

export type EditAction =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "link"
  | "ordered"
  | "bullet"
  | "quote"
  | "code"
  | "pre"
  | "clear"
  | "toolbar"
  | "emoji";
type Binding = { code: string; key: string; shift?: boolean; alt?: boolean };
export const bindings: Record<EditAction, Binding> = {
  bold: { code: "KeyB", key: "B" },
  italic: { code: "KeyI", key: "I" },
  underline: { code: "KeyU", key: "U" },
  strike: { code: "KeyX", key: "X", shift: true },
  link: { code: "KeyU", key: "U", shift: true },
  ordered: { code: "Digit7", key: "7", shift: true },
  bullet: { code: "Digit8", key: "8", shift: true },
  quote: { code: "Digit9", key: "9", shift: true },
  code: { code: "KeyC", key: "C", shift: true },
  // Slack's ⌘⌥⇧C; Alt is AltGr off macOS, so there only the older ⌘⇧J works.
  pre: { code: "KeyC", key: "C", shift: true, alt: true },
  clear: { code: "Backslash", key: "\\" },
  toolbar: { code: "KeyF", key: "F", shift: true },
  emoji: { code: "KeyE", key: "E", shift: true },
};
const aliases: Partial<Record<EditAction, Binding>> = {
  pre: { code: "KeyJ", key: "J", shift: true },
};
/// The binding a platform uses: Alt chords only on macOS.
function binding(action: EditAction, mac: boolean): Binding {
  const b = bindings[action];
  return b.alt && !mac ? (aliases[action] ?? b) : b;
}
export function primaryModifier(e: ShortcutEvent, mac: boolean) {
  return mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}
export function editShortcut(e: ShortcutEvent, mac: boolean): EditAction | undefined {
  if (e.isComposing || (e.altKey && !mac) || !primaryModifier(e, mac)) return;
  const matches = (b: Binding | undefined) =>
    !!b && e.code === b.code && e.shiftKey === !!b.shift && e.altKey === !!b.alt;
  return (Object.keys(bindings) as EditAction[]).find(
    (action) => matches(binding(action, mac)) || matches(aliases[action]),
  );
}
export function shortcutLabel(action: EditAction, mac: boolean) {
  const b = binding(action, mac);
  const p: Platform = mac ? "mac" : "windows";
  return keyCombo(
    [modKeyLabel(p), ...(b.alt ? ["⌥"] : []), ...(b.shift ? [shiftKeyLabel(p)] : []), b.key],
    p,
  );
}
export function ariaShortcut(action: EditAction, mac: boolean) {
  const b = binding(action, mac);
  return [mac ? "Meta" : "Control", ...(b.alt ? ["Alt"] : []), ...(b.shift ? ["Shift"] : []), b.key].join("+");
}
