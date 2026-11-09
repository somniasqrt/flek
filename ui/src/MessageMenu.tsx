import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import Emoji from "./EmojiView";
import EmojiPicker, { quickEmoji } from "./EmojiPicker";

export default function MessageMenu(props: {
  x: number;
  y: number;
  own: string[];
  picker: boolean;
  onReaction: (name: string) => void;
  onClose: () => void;
  onThread?: () => void;
  onCopy: () => void;
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
    (panel.querySelector("input, button") as HTMLElement)?.focus();
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
    if (e.key === "Tab") {
      props.onClose();
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
          role="dialog"
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
                <div class="quick-reactions">
                  <For each={quickEmoji}>
                    {(name) => (
                      <button
                        type="button"
                        title={`:${name}:`}
                        aria-label={`:${name}:`}
                        aria-pressed={props.own.includes(name)}
                        onClick={() => props.onReaction(name)}
                      >
                        <Emoji name={name} />
                      </button>
                    )}
                  </For>
                </div>
                <button class="message-menu-item" type="button" onClick={() => setPicker(true)}>
                  Add reaction… <span>☺</span>
                </button>
                <Show when={props.onThread}>
                  <button
                    class="message-menu-item"
                    type="button"
                    onClick={() => {
                      props.onThread?.();
                      props.onClose();
                    }}
                  >
                    Reply in thread <span>↩</span>
                  </button>
                </Show>
                <button class="message-menu-item" type="button" onClick={props.onCopy}>
                  Copy message <span>⌘C</span>
                </button>
              </>
            }
          >
            <div class="emoji-picker-heading">
              <strong>React to message</strong>
              <button type="button" aria-label="Close emoji picker" onClick={props.onClose}>
                ×
              </button>
            </div>
            <EmojiPicker selected={props.own} onPick={props.onReaction} />
          </Show>
        </div>
      </div>
    </Portal>
  );
}
