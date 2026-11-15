import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api, type Conversation } from "../api";
import { t, errorMessage } from "../i18n/index";
import { channels } from "../stores/people";
import { channelCompletions, personCompletions, type ComposerPerson } from "../models/composer-autocomplete";

export default function ForwardMessage(props: { conversationId: string; ts: string; onClose: () => void; onSent: (warning: boolean) => void }) {
  const [query, setQuery] = createSignal("");
  const [people, setPeople] = createSignal<ComposerPerson[]>([]);
  const [conversations, setConversations] = createSignal<Conversation[]>([]);
  const [destination, setDestination] = createSignal<{ id: string; label: string } | null>(null);
  const [comment, setComment] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  let panel!: HTMLDivElement;
  let input!: HTMLInputElement;
  let alive = true;
  const close = () => !busy() && props.onClose();
  const choices = () => {
    const q = query().trim().replace(/^[@#]/, "");
    const known = new Set(channels().map(c => c.id));
    const directory = [
      ...channels(),
      ...conversations().filter(c => c.kind.endsWith("channel") && !known.has(c.id)).map(c => ({
        id: c.id, name: c.label, is_private: c.kind === "private_channel",
        is_member: c.is_member !== false, is_archived: c.is_archived,
      })),
    ];
    const channelRows = query().startsWith("@") ? [] : channelCompletions(q, directory.filter(c => c.is_member));
    const persons = query().startsWith("#") ? [] : personCompletions(q, people().filter(p => !p.is_bot), false);
    const groups = query().startsWith("#") ? [] : conversations().filter(c => c.kind === "mpim" && !c.is_archived && c.label.toLowerCase().includes(q.toLowerCase()));
    return [...channelRows.map(c => ({ id: c.id, label: `#${c.label}` })), ...persons.map(p => ({ id: p.id, label: p.label })), ...groups.map(c => ({ id: c.id, label: c.label }))].slice(0, 60);
  };
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.focus();
    Promise.all([api.composerPeople(props.conversationId), api.conversations()]).then(([p, c]) => {
      if (alive) { setPeople(p); setConversations(c); }
    }).catch(e => alive && setError(String(e)));
    onCleanup(() => { alive = false; previous?.focus({ preventScroll: true }); });
  });
  const forward = async () => {
    const target = destination();
    if (!target || busy()) return;
    setBusy(true); setError("");
    try {
      const sent = await api.forwardMessage(props.conversationId, props.ts, target.id, comment());
      if (alive) { props.onSent(sent.not_stored_locally); props.onClose(); }
    } catch (e) { if (alive) setError(String(e)); }
    finally { if (alive) setBusy(false); }
  };
  return <Portal>
    <div class="new-message-backdrop" onClick={e => e.target === e.currentTarget && close()}>
      <div ref={panel} class="new-message-picker forward-picker" role="dialog" aria-modal="true" aria-label={t("Forward message…")}
        onKeyDown={e => {
          e.stopPropagation();
          if (e.key === "Escape") { e.preventDefault(); close(); }
          if (e.key === "Tab") {
            const controls = [...panel.querySelectorAll<HTMLElement>("input:not(:disabled),textarea:not(:disabled),button:not(:disabled)")];
            if (!controls.length) { e.preventDefault(); return; }
            if (document.activeElement === (e.shiftKey ? controls[0] : controls.at(-1))) {
              e.preventDefault(); (e.shiftKey ? controls.at(-1) : controls[0])?.focus();
            }
          }
        }}>
        <header><h2>{t("Forward message…")}</h2><button disabled={busy()} onClick={close} aria-label={t("Close")}>×</button></header>
        <p class="muted">{t("Forward a link to this message. Access to the original still applies.")}</p>
        <input ref={input} type="search" disabled={busy()} aria-label={t("Choose a conversation")} placeholder={t("Choose a person or channel…")}
          value={query()} onInput={e => { setQuery(e.currentTarget.value); setDestination(null); }}
          onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); if (choices()[0]) setDestination(choices()[0]); } }} />
        <Show when={destination()} fallback={<>
          <div class="new-message-results"><For each={choices()}>{c => <button disabled={busy()} onClick={() => setDestination(c)}>{c.label}</button>}</For>
            <Show when={!choices().length}><p class="muted">{t("No matching conversations in the local directory.")}</p></Show>
          </div>
        </>}>{target => <p>{t("Forward to {name}", { name: target().label })}</p>}</Show>
        <textarea disabled={busy()} aria-label={t("Add a comment (optional)")} placeholder={t("Add a comment (optional)")} value={comment()} onInput={e => setComment(e.currentTarget.value)} />
        <Show when={error()}><p class="error" role="alert">{errorMessage(error())}</p></Show>
        <button class="primary" disabled={!destination() || busy()} onClick={forward}>{t(busy() ? "Forwarding…" : "Forward")}</button>
      </div>
    </div>
  </Portal>;
}
