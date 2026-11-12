import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { t } from "../i18n/index";
import { api, type Change, type Conversation, type Message } from "../api";
import { Hash, Lock } from "../components/icons";
import Composer from "../components/Composer";
import MessageRow, { repliesLabel } from "../components/MessageRow";
import { pendingFor } from "../stores/outbox";
import type { Jump } from "./Search";

// Right-side thread panel: the root, a "N replies" rule, then the replies.

export default function Thread(props: {
  conversation: Conversation;
  ts: string;
  jump?: Jump | null;
  change: Change | null;
  onClose: () => void;
}) {
  const [rows, setRows] = createStore<Message[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  let scroller!: HTMLDivElement;

  const reveal = () =>
    requestAnimationFrame(() => {
      const ts = props.jump?.ts;
      if (!ts) return;
      scroller.querySelector(`[data-ts="${CSS.escape(ts)}"]`)?.scrollIntoView({ block: "center" });
    });
  createEffect(on(() => props.jump, reveal, { defer: true }));

  const atBottom = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;

  const load = async () => {
    const id = props.conversation.id;
    const ts = props.ts;
    const stick = loaded() && atBottom();
    const list = await api.thread(id, ts);
    // Another thread opened meanwhile.
    if (id !== props.conversation.id || ts !== props.ts) return;
    setRows(reconcile(list, { key: "ts" }));
    setLoaded(true);
    seen();
    if (props.jump?.thread_ts === props.ts) reveal();
    else if (stick) requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollHeight));
  };

  // An open thread in a focused window is read: Activity drops its replies.
  // Slack's own thread cursor API is undocumented, so this stays local.
  const seen = () => {
    const last = rows[rows.length - 1]?.ts;
    if (!last || last === props.ts || !document.hasFocus() || document.hidden) return;
    api.activitySeen(props.conversation.id, props.ts, last).catch(() => {});
  };
  window.addEventListener("focus", seen);
  onCleanup(() => window.removeEventListener("focus", seen));

  createEffect(
    on([() => props.conversation.id, () => props.ts], () => {
      setLoaded(false);
      setRows([]);
      load();
    }),
  );

  // Live: anything touching the root or one of its replies re-reads the thread.
  createEffect(
    on(
      () => props.change,
      async (c) => {
        if (!c || !("data" in c) || !("conversation_id" in c.data)) return;
        const { conversation_id } = c.data;
        if (conversation_id !== props.conversation.id) return;
        if (c.type === "history") return load();
        if (!("ts" in c.data)) return;
        const ts = c.data.ts;
        if (ts === props.ts || rows.some((m) => m.ts === ts)) return load();
        if (c.type !== "message") return;
        const m = await api.message(conversation_id, ts);
        if (m?.thread_ts === props.ts) load();
      },
    ),
  );

  const root = () => (rows[0]?.ts === props.ts ? rows[0] : undefined);
  const stored = () => (root() ? rows.slice(1) : rows.slice());
  // Replies being sent go last, until their stored copy is loaded.
  const pending = createMemo(() => pendingFor(props.conversation.id, props.ts, rows));
  const replies = () => [...stored(), ...pending()];
  const toBottom = () => requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollHeight));
  createEffect(
    on(
      pending,
      () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 150 && toBottom(),
      { defer: true },
    ),
  );
  const kind = () => props.conversation.kind;

  return (
    <aside class="thread-panel" aria-label={t("Thread")}>
      <header class="thread-header">
        <h3>{t("Thread")}</h3>
        <span class="thread-channel muted">
          {kind() === "public_channel" ? (
            <Hash size={13} />
          ) : kind() === "private_channel" ? (
            <Lock size={13} />
          ) : null}
          {props.conversation.label}
        </span>
        <button class="thread-close" type="button" title={t("Close (Esc)")} onClick={props.onClose}>
          <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
            <path d="M5 5l10 10M15 5L5 15" />
          </svg>
        </button>
      </header>
      <div class="thread-scroller" ref={scroller}>
        <Show when={root()}>
          {(m) => <MessageRow m={m()} conversationId={props.conversation.id} />}
        </Show>
        <Show when={replies().length > 0}>
          <div class="thread-divider">
            <span>{repliesLabel(replies().length)}</span>
          </div>
        </Show>
        <For each={replies()}>
          {(m, i) => (
            <MessageRow
              readOnly={props.conversation.is_member === false}
              m={m}
              conversationId={props.conversation.id}
              flash={props.jump?.ts === m.ts}
              prev={i() > 0 ? replies()[i() - 1] : undefined}
            />
          )}
        </For>
        <Show when={loaded() && rows.length === 0}>
          <div class="start muted">{t("This thread is no longer available.")}</div>
        </Show>
        <Show when={loaded() && root() && replies().length === 0}>
          <div class="start muted">{t("Replies appear here once they sync.")}</div>
        </Show>
      </div>
      <Show when={props.conversation.is_member !== false && (!loaded() || rows.length > 0)}>
        <Composer
          conversationId={props.conversation.id}
          threadTs={props.ts}
          placeholder={t("Reply…")}
          broadcastLabel={
            kind() === "im" || kind() === "mpim"
              ? t("Also send as direct message")
              : t("Also send to #{name}", { name: props.conversation.label })
          }
          onSent={toBottom}
        />
      </Show>
    </aside>
  );
}
