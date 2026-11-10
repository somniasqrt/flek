import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import type { Conversation } from "./api";

export default function NewMessage(props: {
  conversations: Conversation[];
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  let input!: HTMLInputElement;
  let panel!: HTMLDivElement;
  const shown = () =>
    props.conversations.filter(
      (c) => !c.is_archived && c.label.toLowerCase().includes(query().trim().toLowerCase()),
    );
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.focus();
    onCleanup(() => previous?.focus({ preventScroll: true }));
  });
  const choose = (id: string) => {
    props.onClose();
    props.onSelect(id);
  };
  return (
    <Portal>
      <div
        class="new-message-backdrop"
        onClick={(e) => e.target === e.currentTarget && props.onClose()}
      >
        <div
          class="new-message-picker"
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-label="New message"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              props.onClose();
            }
            if (e.key === "Tab") {
              const controls = [...panel.querySelectorAll<HTMLElement>("input,button")];
              const i = controls.indexOf(document.activeElement as HTMLElement);
              if (e.shiftKey ? i <= 0 : i === controls.length - 1) {
                e.preventDefault();
                (e.shiftKey ? controls.at(-1) : controls[0])?.focus();
              }
            }
          }}
        >
          <header>
            <h2>New message</h2>
            <button onClick={props.onClose} aria-label="Close">
              ×
            </button>
          </header>
          <input
            ref={input}
            type="search"
            aria-label="Choose a conversation"
            placeholder="Choose a person or channel…"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => e.key === "Enter" && shown()[0] && choose(shown()[0].id)}
          />
          <div class="new-message-results">
            <For each={shown()}>
              {(c) => (
                <button onClick={() => choose(c.id)}>
                  {c.kind.endsWith("channel") ? "#" : ""}
                  {c.label}
                </button>
              )}
            </For>
            <Show when={shown().length === 0}>
              <p class="muted">No matching conversations in the local directory.</p>
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
}
