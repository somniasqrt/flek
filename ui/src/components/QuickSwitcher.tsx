import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import type { Conversation, DirectMessage } from "../api";
import { t } from "../i18n/index";
import { rankConversations } from "../models/switcher";
import { hasUnread } from "../models/conversation-model";
import { people } from "../stores/people";

export default function QuickSwitcher(props: {
  conversations: Conversation[];
  dms: DirectMessage[];
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [index, setIndex] = createSignal(0);
  const results = createMemo(() =>
    rankConversations(props.conversations, query(), props.dms, people()).slice(0, 50),
  );
  let input!: HTMLInputElement;
  let panel!: HTMLDivElement;
  createEffect(() => {
    results();
    setIndex(0);
  });
  createEffect(() => {
    const id = results()[index()]?.id;
    if (id)
      panel
        ?.querySelector(`[data-switch-id="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ block: "nearest" });
  });
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.focus();
    onCleanup(() => previous?.isConnected && previous.focus({ preventScroll: true }));
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
          class="new-message-picker quick-switcher"
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-label={t("Quick switcher")}
          onKeyDown={(e) => {
            if (e.isComposing) return;
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              props.onClose();
            }
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              setIndex(
                (i) =>
                  (i + (e.key === "ArrowDown" ? 1 : -1) + results().length) %
                  (results().length || 1),
              );
            }
            if (e.key === "Enter" && results()[index()]) {
              e.preventDefault();
              choose(results()[index()].id);
            }
            if (e.key === "Tab") {
              e.preventDefault();
              input.focus();
            }
          }}
        >
          <header>
            <h2>{t("Quick switcher")}</h2>
            <button tabIndex={-1} onClick={props.onClose} aria-label={t("Close")}>
              ×
            </button>
          </header>
          <input
            ref={input}
            type="search"
            role="combobox"
            aria-expanded="true"
            aria-controls="switcher-results"
            aria-autocomplete="list"
            aria-activedescendant={
              results()[index()] ? `switch-${results()[index()].id}` : undefined
            }
            aria-label={t("Find a conversation…")}
            placeholder={t("Find a conversation…")}
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
          />
          <div class="new-message-results" id="switcher-results" role="listbox">
            <For each={results()}>
              {(c, i) => (
                <button
                  role="option"
                  tabIndex={-1}
                  id={`switch-${c.id}`}
                  data-switch-id={c.id}
                  aria-selected={i() === index()}
                  classList={{ unread: hasUnread(c) }}
                  onMouseMove={() => setIndex(i())}
                  onClick={() => choose(c.id)}
                >
                  {c.kind.endsWith("channel") ? "# " : ""}
                  {c.label}
                  <Show when={hasUnread(c)}>
                    <span class="badge">{c.unread}</span>
                  </Show>
                </button>
              )}
            </For>
            <Show when={!results().length}>
              <p class="muted">{t("No conversations match")}</p>
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
}
