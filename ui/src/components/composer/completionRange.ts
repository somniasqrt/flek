import { detectCompletion } from "../../models/composer-autocomplete.ts";

// Build a text prefix with explicit block/atomic boundaries, retaining DOM offsets
// so replacement works even when a query spans bold/italic or adjacent text nodes.
export function completionRange(editor: HTMLElement) {
  const selection = window.getSelection();
  if (!selection?.isCollapsed || !selection.rangeCount) return null;
  const caret = selection.getRangeAt(0);
  if (!editor.contains(caret.startContainer)) return null;
  const element =
    caret.startContainer instanceof Element
      ? caret.startContainer
      : caret.startContainer.parentElement;
  if (element?.closest('code, pre, [contenteditable="false"]')) return null;
  let text = "",
    stopped = false;
  const segments: { node: Text; start: number; length: number }[] = [];
  const walk = (node: Node) => {
    if (stopped) return;
    if (node.nodeType === Node.TEXT_NODE) {
      const value = (node.textContent || "").slice(
        0,
        node === caret.startContainer ? caret.startOffset : undefined,
      );
      segments.push({ node: node as Text, start: text.length, length: value.length });
      text += value;
      if (node === caret.startContainer) stopped = true;
    } else if (node instanceof HTMLElement) {
      if (
        node !== editor &&
        (node.contentEditable === "false" || ["BR", "CODE", "PRE"].includes(node.tagName))
      ) {
        text += node.tagName === "BR" || node.tagName === "PRE" ? "\n" : "\ufffc";
        return;
      }
      const block = /^(DIV|P|LI|UL|OL|BLOCKQUOTE|PRE)$/.test(node.tagName);
      if (block && node !== editor) text += "\n";
      for (let i = 0; i <= node.childNodes.length; i++) {
        if (node === caret.startContainer && i === caret.startOffset) {
          stopped = true;
          break;
        }
        if (i < node.childNodes.length) walk(node.childNodes[i]);
        if (stopped) break;
      }
      if (block && !stopped) text += "\n";
    }
  };
  walk(editor);
  const token = detectCompletion(text);
  if (!token) return null;
  const start = segments.find((s) => token.start >= s.start && token.start < s.start + s.length);
  if (!start) return null;
  const range = caret.cloneRange();
  range.setStart(start.node, token.start - start.start);
  return { token, range };
}
