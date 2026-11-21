import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api, type Conversation } from "../api";
import { errorMessage, t } from "../i18n/index";
import type { ComposerPerson } from "../models/composer-autocomplete";
import {
  MAX_DM_PEOPLE,
  newMessageRows,
  type NewMessageRow,
  type SelectedPerson,
} from "../models/new-message";

export default function NewMessage(props: {
  conversations: Conversation[];
  me: string;
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [people, setPeople] = createSignal<ComposerPerson[]>([]);
  const [selected, setSelected] = createSignal<SelectedPerson[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  let input!: HTMLInputElement;
  let panel!: HTMLDivElement;
  let alive = true;

  const close = () => !busy() && props.onClose();
  const rows = () => newMessageRows(query(), selected(), people(), props.conversations, props.me);

  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.focus();
    api
      .composerPeople("")
      .then((list) => {
        if (alive) setPeople(list);
      })
      .catch((err) => {
        if (alive) setError(String(err));
      });
    onCleanup(() => {
      alive = false;
      previous?.focus({ preventScroll: true });
    });
  });

  const chooseConversation = (id: string) => {
    props.onClose();
    props.onSelect(id);
  };

  const addPerson = (person: SelectedPerson) => {
    if (selected().length >= MAX_DM_PEOPLE || selected().some((p) => p.id === person.id)) return;
    setSelected((list) => [...list, person]);
    setQuery("");
    setError("");
    input.focus();
  };

  const removePerson = (id: string) => {
    setSelected((list) => list.filter((p) => p.id !== id));
    setError("");
    input.focus();
  };

  const pickRow = (row: NewMessageRow) => {
    if (busy()) return;
    if (row.kind === "conversation") {
      chooseConversation(row.id);
    } else {
      addPerson({ id: row.id, label: row.label });
    }
  };

  const openSelected = async () => {
    const ids = selected().map((p) => p.id);
    if (!ids.length || busy()) return;
    setBusy(true);
    setError("");
    try {
      const result = await api.openDirectMessage(ids);
      if (alive) {
        props.onClose();
        props.onSelect(result.id);
      }
    } catch (err) {
      if (alive) setError(String(err));
    } finally {
      if (alive) setBusy(false);
    }
  };

  return (
    <Portal>
      <div
        class="new-message-backdrop"
        onClick={(e) => e.target === e.currentTarget && close()}
      >
        <div
          class="new-message-picker"
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-label={t("New message")}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              e.preventDefault();
              close();
            }
            if (e.key === "Tab") {
              const controls = [
                ...panel.querySelectorAll<HTMLElement>("input:not(:disabled),button:not(:disabled)"),
              ];
              if (!controls.length) {
                e.preventDefault();
                return;
              }
              const i = controls.indexOf(document.activeElement as HTMLElement);
              if (e.shiftKey ? i <= 0 : i === controls.length - 1) {
                e.preventDefault();
                (e.shiftKey ? controls.at(-1) : controls[0])?.focus();
              }
            }
          }}
        >
          <header>
            <h2>{t("New message")}</h2>
            <button type="button" disabled={busy()} onClick={close} aria-label={t("Close")}>
              ×
            </button>
          </header>
          <Show when={selected().length > 0}>
            <div class="new-message-to">
              <span class="new-message-to-label">{t("To:")}</span>
              <For each={selected()}>
                {(person) => (
                  <span class="new-message-chip">
                    <span>{person.label}</span>
                    <button
                      type="button"
                      disabled={busy()}
                      aria-label={t("Remove {name}", { name: person.label })}
                      onClick={() => removePerson(person.id)}
                    >
                      ×
                    </button>
                  </span>
                )}
              </For>
            </div>
          </Show>
          <input
            ref={input}
            type="search"
            disabled={busy()}
            aria-label={
              selected().length > 0 ? t("Add another person…") : t("Choose a conversation")
            }
            placeholder={
              selected().length > 0 ? t("Add another person…") : t("Choose a person or channel…")
            }
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Backspace" && !query() && selected().length > 0 && !busy()) {
                setSelected((list) => list.slice(0, -1));
                setError("");
                return;
              }
              if (e.key === "Enter" && !busy()) {
                e.preventDefault();
                if (!query().trim() && selected().length > 0) {
                  void openSelected();
                } else if (rows()[0]) {
                  pickRow(rows()[0]);
                }
              }
            }}
          />
          <Show when={error()}>
            <p class="error" role="alert">
              {errorMessage(error())}
            </p>
          </Show>
          <div class="new-message-results">
            <For each={rows()}>
              {(row) => (
                <button type="button" disabled={busy()} onClick={() => pickRow(row)}>
                  {row.kind === "conversation"
                    ? `${row.conversationKind.endsWith("channel") ? "#" : ""}${row.label}`
                    : row.label}
                </button>
              )}
            </For>
            <Show when={rows().length === 0 && selected().length < MAX_DM_PEOPLE}>
              <p class="muted">{t("No matching conversations in the local directory.")}</p>
            </Show>
          </div>
          <Show when={selected().length > 0}>
            <div class="new-message-actions">
              <button type="button" class="primary" disabled={busy()} onClick={openSelected}>
                {busy() ? t("Opening…") : t("Open")}
              </button>
            </div>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
