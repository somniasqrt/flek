import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { primaryModifier, usesMacShortcuts } from "./shortcuts";
import { AddReaction, ReplyInThread, MessageLink, CopyMessage } from "./icons";
import EmojiPicker from "./EmojiPicker";

export default function MessageMenu(props: {
  x: number;
  y: number;
  own: string[];
  picker: boolean;
  onReaction: (name: string) => void;
  onClose: () => void;
  onThread?: () => void;
  onCopy: () => void;
  onCopyLink: () => void;
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
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
    if ((e.target as HTMLElement).tagName !== "INPUT" && !picker()) {
      const key = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey ? e.code : "";
      if (key === "KeyR") {
        e.preventDefault();
        setPicker(true);
        return;
      }
      if (key === "KeyT" && props.onThread) {
        e.preventDefault();
        props.onThread();
        props.onClose();
        return;
      }
      if (key === "KeyL") {
        e.preventDefault();
        props.onCopyLink();
        return;
      }
      if (primaryModifier(e, usesMacShortcuts()) && e.code === "KeyC") {
        e.preventDefault();
        props.onCopy();
        return;
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
    const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button")];
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
          aria-label={picker() ? "Choose a reaction" : "Message actions"}
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
                  onClick={() => setPicker(true)}
                >
                  <AddReaction />
                  <span class="message-menu-label">Add reaction…</span>
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
                    <span class="message-menu-label">Reply in thread</span>
                    <kbd>T</kbd>
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
                  <span class="message-menu-label">Copy link</span>
                  <kbd>L</kbd>
                </button>
                <button
                  class="message-menu-item"
                  role="menuitem"
                  type="button"
                  onClick={props.onCopy}
                >
                  <CopyMessage />
                  <span class="message-menu-label">Copy message</span>
                  <kbd>{usesMacShortcuts() ? "⌘C" : "Ctrl+C"}</kbd>
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
