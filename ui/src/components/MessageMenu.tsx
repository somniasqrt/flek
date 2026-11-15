import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "../i18n/index";
import { keyCombo, modKeyLabel, shiftKeyLabel } from "../platform";
import { usesMacShortcuts } from "../models/shortcuts";
import { AddReaction, ReplyInThread, MessageLink, CopyMessage } from "./icons";
import { messageActionKey } from "../models/message-edit";
import { messageShortcut } from "../models/message-actions";
import type { MessageActionState } from "../api";
import EmojiPicker from "./EmojiPicker";

export default function MessageMenu(props: {
  x: number;
  y: number;
  own: string[];
  picker: boolean;
  readOnly?: boolean;
  onReaction: (name: string) => void;
  onClose: () => void;
  onThread?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
  onCopy: () => void;
  onCopyLink: () => void;
  onOpenSlack: () => void;
  actionState: MessageActionState | null;
  busy: boolean;
  onForward: () => void;
  onUnread: () => void;
  onSave: () => void;
  onPin: () => void;
}) {
  const [picker, setPicker] = createSignal(props.picker);
  const [position, setPosition] = createSignal({ left: props.x, top: props.y });
  let panel!: HTMLDivElement;
  const fit = () =>
    setPosition({
      left: Math.max(8, Math.min(props.x, window.innerWidth - panel.offsetWidth - 8)),
      top: Math.max(8, Math.min(props.y, window.innerHeight - panel.offsetHeight - 8)),
    });
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const observer = new ResizeObserver(fit);
    observer.observe(panel);
    fit();
    (panel.querySelector(props.picker ? "input" : "button") as HTMLElement)?.focus();
    const dismiss = () => props.onClose();
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
      previous?.focus({ preventScroll: true });
    });
  });
  const keydown = (e: KeyboardEvent) => {
    if (e.isComposing || e.keyCode === 229) return;
    if (!picker() && !(e.target as Element).closest("input, textarea, [contenteditable=true]")) {
      const action = messageActionKey(e);
      const handler = action === "edit" ? props.onEdit : action === "delete" ? props.onDelete : undefined;
      if (handler && !props.readOnly) {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
        handler();
        return;
      }
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
    if ((e.target as HTMLElement).tagName !== "INPUT" && !picker()) {
      const action = messageShortcut(e, usesMacShortcuts());
      const handlers = {
        reaction: !props.readOnly ? () => setPicker(true) : undefined,
        thread: props.onThread ? () => { props.onThread?.(); props.onClose(); } : undefined,
        forward: props.onForward,
        unread: !props.readOnly ? props.onUnread : undefined,
        save: props.actionState ? props.onSave : undefined,
        pin: !props.readOnly && props.actionState ? props.onPin : undefined,
        link: props.onCopyLink,
        copy: props.onCopy,
      };
      if (action && handlers[action] && !props.busy) {
        e.preventDefault(); e.stopPropagation(); handlers[action]?.(); return;
      }
    }
    if (e.key === "Tab") {
      const controls = [...panel.querySelectorAll<HTMLElement>("input, button:not(:disabled)")];
      const target = e.shiftKey ? controls.at(-1) : controls[0];
      if (document.activeElement === (e.shiftKey ? controls[0] : controls.at(-1))) {
        e.preventDefault();
        target?.focus();
      }
      return;
    }
    if (
      !["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(e.key) ||
      (e.target as HTMLElement).tagName === "INPUT"
    )
      return;
    e.preventDefault();
    const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[
      (index + (e.key === "ArrowUp" || e.key === "ArrowLeft" ? -1 : 1) + buttons.length) %
        buttons.length
    ]?.focus();
  };
  return (
    <Portal>
      <div
        class="message-menu-backdrop"
        onPointerDown={() => props.onClose()}
        onContextMenu={(e) => {
          e.preventDefault();
          props.onClose();
        }}
      >
        <div
          ref={panel}
          class="message-menu"
          classList={{ "emoji-picker-menu": picker() }}
          role={picker() ? "dialog" : "menu"}
          aria-label={picker() ? t("Choose a reaction") : t("Message actions")}
          style={{ left: `${position().left}px`, top: `${position().top}px` }}
          onPointerDown={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
          onKeyDown={keydown}
        >
          <Show
            when={picker()}
            fallback={
              <>
                <button
                  class="message-menu-item"
                  role="menuitem"
                  type="button"
                  disabled={props.readOnly}
                  onClick={() => setPicker(true)}
                >
                  <AddReaction />
                  <span class="message-menu-label">{t("Add reaction…")}</span>
                  <kbd>R</kbd>
                </button>
                <Show when={props.onThread}>
                  <button
                    class="message-menu-item"
                    role="menuitem"
                    type="button"
                    onClick={() => {
                      props.onThread?.();
                      props.onClose();
                    }}
                  >
                    <ReplyInThread />
                    <span class="message-menu-label">{t("Reply in thread")}</span>
                    <kbd>T</kbd>
                  </button>
                </Show>
                <button class="message-menu-item" role="menuitem" type="button" onClick={props.onForward} disabled={props.busy}>
                  <MessageLink /><span class="message-menu-label">{t("Forward message…")}</span><kbd>{keyCombo(["F"])}</kbd>
                </button>
                <button class="message-menu-item" role="menuitem" type="button" onClick={props.onUnread} disabled={props.busy || props.readOnly}>
                  <span aria-hidden="true">◉</span><span class="message-menu-label">{t("Mark unread")}</span><kbd>{keyCombo(["U"])}</kbd>
                </button>
                <button class="message-menu-item" role="menuitem" type="button" onClick={props.onSave} disabled={props.busy || !props.actionState}>
                  <span aria-hidden="true">☆</span><span class="message-menu-label">{t(props.actionState?.saved ? "Remove from saved" : "Add to saved")}</span><kbd>{keyCombo([modKeyLabel(), shiftKeyLabel(), "S"])}</kbd>
                </button>
                <p class="message-menu-note">{t("Saved in Flek; Slack Later is unavailable.")}</p>
                <button class="message-menu-item" role="menuitem" type="button" onClick={props.onOpenSlack}>
                  <MessageLink /><span class="message-menu-label">{t("Open in Slack")}</span>
                </button>
                <Show when={props.onEdit && !props.readOnly}>
                  <button class="message-menu-item" role="menuitem" type="button" onClick={() => { props.onClose(); props.onEdit?.(); }}>
                    <span aria-hidden="true">✎</span><span class="message-menu-label">{t("Edit message")}</span><kbd>{keyCombo(["E"])}</kbd>
                  </button>
                </Show>
                <Show when={props.onDelete && !props.readOnly}>
                  <button class="message-menu-item danger" role="menuitem" type="button" onClick={() => { props.onClose(); props.onDelete?.(); }}>
                    <span aria-hidden="true">×</span><span class="message-menu-label">{t("Delete message…")}</span><kbd>{usesMacShortcuts() ? "⌫" : keyCombo(["Delete"])}</kbd>
                  </button>
                </Show>
                <div class="message-menu-rule" role="separator" />
                <button
                  class="message-menu-item"
                  role="menuitem"
                  type="button"
                  onClick={props.onCopyLink}
                >
                  <MessageLink />
                  <span class="message-menu-label">{t("Copy link")}</span>
                  <kbd>L</kbd>
                </button>
                <button
                  class="message-menu-item"
                  role="menuitem"
                  type="button"
                  onClick={props.onCopy}
                >
                  <CopyMessage />
                  <span class="message-menu-label">{t("Copy message")}</span>
                  <kbd>{keyCombo([modKeyLabel(), "C"])}</kbd>
                </button>
                <button class="message-menu-item" role="menuitem" type="button" onClick={props.onPin} disabled={props.busy || props.readOnly || !props.actionState}>
                  <span aria-hidden="true">⌖</span><span class="message-menu-label">{t(props.actionState?.pinned ? "Unpin from this conversation" : "Pin to this conversation")}</span><kbd>{keyCombo(["P"])}</kbd>
                </button>
              </>
            }
          >
            <EmojiPicker selected={props.own} onPick={props.onReaction} />
          </Show>
        </div>
      </div>
    </Portal>
  );
}
