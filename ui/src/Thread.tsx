// Right-side thread panel: the root, a "N replies" rule, then the replies.
import { createEffect, createSignal, For, on, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { api, type Change, type Conversation, type Message } from "./api";
import { Hash, Lock } from "./icons";
import MessageRow, { repliesLabel } from "./MessageRow";

export default function Thread(props: {
  conversation: Conversation;
  ts: string;
  change: Change | null;
  onClose: () => void;
}) {
  const [rows, setRows] = createStore<Message[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  let scroller!: HTMLDivElement;

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
    if (stick) requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollHeight));
  };

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
  const replies = () => (root() ? rows.slice(1) : rows.slice());
  const kind = () => props.conversation.kind;

  return (
    <aside class="thread-panel" aria-label="Thread">
      <header class="thread-header">
        <h3>Thread</h3>
        <span class="thread-channel muted">
          {kind() === "public_channel" ? (
            <Hash size={13} />
          ) : kind() === "private_channel" ? (
            <Lock size={13} />
          ) : null}
          {props.conversation.label}
        </span>
        <button class="thread-close" type="button" title="Close (Esc)" onClick={props.onClose}>
          <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
            <path d="M5 5l10 10M15 5L5 15" />
          </svg>
        </button>
      </header>
      <div class="thread-scroller" ref={scroller}>
        <Show when={root()}>{(m) => <MessageRow m={m()} />}</Show>
        <Show when={replies().length > 0}>
          <div class="thread-divider">
            <span>{repliesLabel(replies().length)}</span>
          </div>
        </Show>
        <For each={replies()}>
          {(m, i) => <MessageRow m={m} prev={i() > 0 ? replies()[i() - 1] : undefined} />}
        </For>
        <Show when={loaded() && rows.length === 0}>
          <div class="start muted">This thread is no longer available.</div>
        </Show>
        <Show when={loaded() && root() && replies().length === 0}>
          <div class="start muted">Replies appear here once they sync.</div>
        </Show>
      </div>
    </aside>
  );
}
