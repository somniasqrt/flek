import { createEffect, createMemo, createSignal, For, on, Show } from "solid-js";
import { createStore, reconcile, unwrap } from "solid-js/store";
import { api, isReplyOnly, tsDate, type Change, type Conversation, type Message } from "./api";
import { Hash, Lock } from "./icons";
import MessageRow from "./MessageRow";

const PAGE = 60;
/// Load older messages when the top is this close (px).
const LOAD_MARGIN = 400;

export default function Timeline(props: {
  conversation: Conversation;
  change: Change | null;
  /// Root ts of the thread open in the side panel.
  thread: string | null;
  onThread: (ts: string) => void;
}) {
  // Reconciled by ts: updates touch only the rows that changed, so syncing
  // never re-renders (and flickers) the whole timeline.
  const [rows, setRows] = createStore<Message[]>([]);
  const show = (list: Message[]) => setRows(reconcile(list, { key: "ts" }));
  const current = () => unwrap(rows).slice();
  const [loading, setLoading] = createSignal(false);
  const [exhausted, setExhausted] = createSignal(false);
  let scroller!: HTMLDivElement;
  // Bumped on every switch so late pages of the previous conversation are dropped.
  let generation = 0;

  const atBottom = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
  const toBottom = () => requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollHeight));

  createEffect(
    on(
      () => props.conversation.id,
      async (id) => {
        const gen = ++generation;
        show([]);
        setExhausted(false);
        setLoading(true);
        const page = await api.messages(id, null, PAGE);
        if (gen !== generation) return;
        show(page);
        setExhausted(page.length < PAGE);
        setLoading(false);
        toBottom();
      },
    ),
  );

  const loadOlder = async () => {
    if (loading() || exhausted() || rows.length === 0) return;
    const gen = generation;
    setLoading(true);
    const page = await api.messages(props.conversation.id, rows[0].ts, PAGE);
    if (gen !== generation) return;
    const before = scroller.scrollHeight;
    show([...page, ...current()]);
    setExhausted(page.length < PAGE);
    setLoading(false);
    // Keep what the user is looking at in place.
    scroller.scrollTop += scroller.scrollHeight - before;
  };

  createEffect(
    on(
      () => props.change,
      async (c) => {
        if (!c || !("data" in c) || !("conversation_id" in c.data)) return;
        if (c.data.conversation_id !== props.conversation.id) return;
        if (c.type === "history") return refreshNewest();
        if (!("ts" in c.data)) return;
        const { conversation_id, ts } = c.data;
        if (c.type === "message_deleted") {
          show(current().filter((m) => m.ts !== ts));
          return;
        }
        const stick = atBottom();
        const m = await api.message(conversation_id, ts);
        if (conversation_id !== props.conversation.id) return;
        if (!m) {
          show(current().filter((x) => x.ts !== ts));
        } else if (isReplyOnly(m)) {
          // A reply changes its root's reply count.
          const root = await api.message(conversation_id, m.thread_ts!);
          if (root) upsert(root);
        } else {
          upsert(m);
          if (stick) toBottom();
        }
      },
    ),
  );

  const onScroll = () => scroller.scrollTop < LOAD_MARGIN && loadOlder();

  // Messages split by day. Day keys are strings, so <For> keeps a day's
  // section (and its rows) when other days change or older ones load.
  const byDay = createMemo(() => {
    const map = new Map<string, Message[]>();
    for (const m of rows) {
      const k = dayKey(m.ts);
      const list = map.get(k);
      if (list) list.push(m);
      else map.set(k, [m]);
    }
    return map;
  });
  const days = createMemo(() => [...byDay().keys()]);

  const upsert = (m: Message) => {
    const r = current();
    const i = r.findIndex((x) => x.ts === m.ts);
    if (i >= 0) r[i] = m;
    // Older than everything loaded: it shows up when scrolling back.
    else if (r.length > 0 && m.ts < r[0].ts) return;
    else r.push(m);
    show(r.sort((a, b) => a.ts.localeCompare(b.ts)));
  };

  /// Sync wrote into this conversation: re-read the newest page and keep the
  /// older rows already loaded.
  const refreshNewest = async () => {
    const id = props.conversation.id;
    const gen = generation;
    const stick = atBottom();
    const page = await api.messages(id, null, PAGE);
    if (gen !== generation) return;
    const kept = page.length < PAGE ? [] : current().filter((m) => m.ts < page[0].ts);
    show([...kept, ...page]);
    if (page.length < PAGE) setExhausted(true);
    if (stick) toBottom();
  };

  return (
    <div class="timeline">
      <header class="conv-header">
        <h2>
          {props.conversation.kind === "public_channel" ? (
            <Hash size={18} />
          ) : props.conversation.kind === "private_channel" ? (
            <Lock size={18} />
          ) : null}
          {props.conversation.label}
        </h2>
        <Show when={props.conversation.topic}>
          <span class="muted topic">{props.conversation.topic}</span>
        </Show>
      </header>
      <div class="scroll-area">
      <div class="scroller" ref={scroller} onScroll={onScroll}>
        <Show when={exhausted() && rows.length > 0}>
          <div class="start muted">Start of what Flek has for this conversation</div>
        </Show>
        <Show when={!loading() && rows.length === 0}>
          <div class="start muted">No messages yet. They appear here as they sync.</div>
        </Show>
        <For each={days()}>
          {(key) => {
            const items = () => byDay().get(key) ?? [];
            return (
              // Each day is its own section, so its sticky label only sticks
              // while that day is on screen (like Slack).
              <section class="day-section">
                <div class="day">
                  <span>{dayLabel(items()[0]?.ts ?? "0")}</span>
                </div>
                <For each={items()}>{(m, i) => (
                    <MessageRow
                      m={m}
                      prev={items()[i() - 1]}
                      onThread={props.onThread}
                      active={props.thread === m.ts}
                    />
                  )}</For>
              </section>
            );
          }}
        </For>
      </div>
      </div>
    </div>
  );
}

const dayKey = (ts: string) => tsDate(ts).toDateString();

function dayLabel(ts: string): string {
  const d = tsDate(ts);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return "Today";
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: d.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}
