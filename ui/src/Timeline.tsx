import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { createStore, reconcile, unwrap } from "solid-js/store";
import { api, isReplyOnly, tsDate, type Change, type Conversation, type Message } from "./api";
import { Hash, Lock } from "./icons";
import Composer from "./Composer";
import MessageRow from "./MessageRow";
import { pendingFor } from "./outbox";
import type { Jump } from "./Search";

const PAGE = 60;
/// Load older messages when the top is this close (px).
const LOAD_MARGIN = 400;
/// Revealing an older message loads history in pages this big, at most
/// REVEAL_PAGES of them (the list is not virtualized yet).
const REVEAL_PAGE = 500;
const REVEAL_PAGES = 20;

export default function Timeline(props: {
  conversation: Conversation;
  change: Change | null;
  /// Root ts of the thread open in the side panel.
  thread: string | null;
  onThread: (ts: string) => void;
  /// A message to scroll to and highlight (a search result).
  jump: Jump | null;
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
  const nearBottom = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 150;
  const toBottom = () => requestAnimationFrame(() => (scroller.scrollTop = scroller.scrollHeight));

  const [flash, setFlash] = createSignal<string | null>(null);
  let flashTimer: number | undefined;
  onCleanup(() => window.clearTimeout(flashTimer));

  createEffect(
    on(
      () => [props.conversation.id, props.jump] as const,
      async ([id, jump], prev) => {
        const target = jump && jump.conversation_id === id ? jump : null;
        // Same conversation, new target: no reload.
        if (prev && prev[0] === id) {
          if (target) await reveal(target);
          return;
        }
        const gen = ++generation;
        show([]);
        setExhausted(false);
        setLoading(true);
        const page = await api.messages(id, null, PAGE);
        if (gen !== generation) return;
        show(page);
        setExhausted(page.length < PAGE);
        setLoading(false);
        if (target) await reveal(target);
        else toBottom();
      },
    ),
  );

  /// Loads older history until the target is in, then centres and briefly
  /// highlights it. A reply that is only in its thread shows as its root.
  const reveal = async (t: Jump) => {
    const gen = generation;
    const id = props.conversation.id;
    const oldest = t.thread_ts && t.thread_ts < t.ts ? t.thread_ts : t.ts;
    setLoading(true);
    for (let i = 0; i < REVEAL_PAGES && !exhausted() && rows.length > 0 && rows[0].ts > oldest; i++) {
      const page = await api.messages(id, rows[0].ts, REVEAL_PAGE);
      if (gen !== generation) return;
      show([...page, ...current()]);
      setExhausted(page.length < REVEAL_PAGE);
    }
    setLoading(false);
    const find = (ts: string) =>
      scroller.querySelector<HTMLElement>(`[data-ts="${CSS.escape(ts)}"]`);
    const el = find(t.ts) ?? (t.thread_ts ? find(t.thread_ts) : null);
    if (!el) return toBottom();
    el.scrollIntoView({ block: "center" });
    setFlash(el.dataset.ts ?? null);
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => setFlash(null), 2500);
  };

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
          // A broadcast reply also changes its root's reply count.
          if (m.thread_ts && m.thread_ts !== m.ts) {
            const root = await api.message(conversation_id, m.thread_ts);
            if (root && conversation_id === props.conversation.id) upsert(root);
          }
        }
      },
    ),
  );

  const onScroll = () => scroller.scrollTop < LOAD_MARGIN && loadOlder();

  // Messages split by day. Day keys are strings, so <For> keeps a day's
  // section (and its rows) when other days change or older ones load.
  // Messages being sent go last, until their stored copy is in `rows`.
  const pending = createMemo(() => pendingFor(props.conversation.id, null, rows));
  // A pending row that grows ("Not sent · Retry") stays in view at the bottom.
  createEffect(on(pending, () => nearBottom() && toBottom(), { defer: true }));
  const byDay = createMemo(() => {
    const map = new Map<string, Message[]>();
    for (const m of [...rows, ...pending()]) {
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
                <For each={items()}>
                  {(m, i) => (
                    <MessageRow
                      m={m}
                      prev={items()[i() - 1]}
                      onThread={props.onThread}
                      active={props.thread === m.ts}
                      flash={flash() === m.ts}
                    />
                  )}
                </For>
              </section>
            );
          }}
        </For>
      </div>
      </div>
      <Composer
        conversationId={props.conversation.id}
        placeholder={`Message ${isChannel(props.conversation) ? "#" : ""}${props.conversation.label}`}
        onSent={toBottom}
      />
    </div>
  );
}

const isChannel = (c: Conversation) => c.kind === "public_channel" || c.kind === "private_channel";

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
