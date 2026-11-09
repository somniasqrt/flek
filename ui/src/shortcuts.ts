// Physical key codes keep shortcuts working with Ukrainian and other layouts.
export type ShortcutEvent = Pick<
  KeyboardEvent,
  "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing"
>;
export const isMacPlatform = (platform: string) => /mac|iphone|ipad/i.test(platform);
export const usesMacShortcuts = () => isMacPlatform(navigator.platform || navigator.userAgent);

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
export const bindings: Record<EditAction, { code: string; key: string; shift?: boolean }> = {
  bold: { code: "KeyB", key: "B" },
  italic: { code: "KeyI", key: "I" },
  underline: { code: "KeyU", key: "U" },
  strike: { code: "KeyX", key: "X", shift: true },
  link: { code: "KeyK", key: "K" },
  ordered: { code: "Digit7", key: "7", shift: true },
  bullet: { code: "Digit8", key: "8", shift: true },
  quote: { code: "Digit9", key: "9", shift: true },
  code: { code: "KeyC", key: "C", shift: true },
  pre: { code: "KeyJ", key: "J", shift: true },
  clear: { code: "Backslash", key: "\\" },
  toolbar: { code: "KeyF", key: "F", shift: true },
  emoji: { code: "KeyE", key: "E", shift: true },
};
export function primaryModifier(e: ShortcutEvent, mac: boolean) {
  return mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}
export function editShortcut(e: ShortcutEvent, mac: boolean): EditAction | undefined {
  if (e.isComposing || e.altKey || !primaryModifier(e, mac)) return;
  return (Object.keys(bindings) as EditAction[]).find((action) => {
    const b = bindings[action];
    return e.code === b.code && e.shiftKey === !!b.shift;
  });
}
export function shortcutLabel(action: EditAction, mac: boolean) {
  const b = bindings[action];
  return mac ? `⌘${b.shift ? "⇧" : ""}${b.key}` : `Ctrl+${b.shift ? "Shift+" : ""}${b.key}`;
}
