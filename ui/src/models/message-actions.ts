import { primaryModifier, type ShortcutEvent } from "./shortcuts.ts";

export type MessageAction = "reaction" | "thread" | "forward" | "unread" | "save" | "pin" | "link" | "copy";
const letterActions: Record<string, MessageAction> = { KeyR: "reaction", KeyT: "thread", KeyF: "forward", KeyU: "unread", KeyP: "pin", KeyL: "link" };
export function messageShortcut(e: ShortcutEvent, mac: boolean): MessageAction | undefined {
  if (e.isComposing || e.altKey) return;
  if (primaryModifier(e, mac)) {
    if (e.code === "KeyC" && !e.shiftKey) return "copy";
    if (e.code === "KeyS" && e.shiftKey) return "save";
    return;
  }
  if (e.metaKey || e.ctrlKey || e.shiftKey) return;
  return letterActions[e.code];
}
