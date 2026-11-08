import { createEffect, createSignal, For, on, Show } from "solid-js";
import { api, isReplyOnly, tsDate, type Change, type Conversation, type Message } from "./api";

const PAGE = 60;
/// Load older messages when the top is this close (px).
const LOAD_MARGIN = 400;
/// Messages from one author closer than this are grouped (seconds).
const GROUP_GAP = 5 * 60;

export default function Timeline(props: { conversation: Conversation; change: Change | null }) {
  const [rows, setRows] = createSignal<Message[]>([]);
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
        setRows([]);
        setExhausted(false);
        setLoading(true);
        const page = await api.messages(id, null, PAGE);
        if (gen !== generation) return;
        setRows(page);
        setExhausted(page.length < PAGE);
        setLoading(false);
        toBottom();
      },
    ),
  );

  const loadOlder = async () => {
    if (loading() || exhausted() || rows().length === 0) return;
    const gen = generation;
    setLoading(true);
    const page = await api.messages(props.conversation.id, rows()[0].ts, PAGE);
    if (gen !== generation) return;
    const before = scroller.scrollHeight;
    setRows((r) => [...page, ...r]);
    setExhausted(page.length < PAGE);
    setLoading(false);
    // Keep what the user is looking at in place.
    scroller.scrollTop += scroller.scrollHeight - before;
  };

  createEffect(
    on(
      () => props.change,
      async (c) => {
        if (!c || !("data" in c) || !("ts" in c.data)) return;
        if (c.data.conversation_id !== props.conversation.id) return;
        const { conversation_id, ts } = c.data;
        if (c.type === "message_deleted") {
          setRows((r) => r.filter((m) => m.ts !== ts));
          return;
        }
        const stick = atBottom();
        const m = await api.message(conversation_id, ts);
        if (conversation_id !== props.conversation.id) return;
        if (!m) {
          setRows((r) => r.filter((x) => x.ts !== ts));
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

  const upsert = (m: Message) =>
    setRows((r) => {
      const i = r.findIndex((x) => x.ts === m.ts);
      if (i >= 0) return [...r.slice(0, i), m, ...r.slice(i + 1)];
      // Older than everything loaded: it shows up when scrolling back.
      if (r.length > 0 && m.ts < r[0].ts) return r;
      return [...r, m].sort((a, b) => a.ts.localeCompare(b.ts));
    });

  return (
    <div class="timeline">
      <header class="conv-header">
        <h2>
          {props.conversation.kind === "public_channel"
            ? "#"
            : props.conversation.kind === "private_channel"
              ? "🔒 "
              : ""}
          {props.conversation.label}
        </h2>
        <Show when={props.conversation.topic}>
          <span class="muted topic">{props.conversation.topic}</span>
        </Show>
      </header>
      <div
        class="scroller"
        ref={scroller}
        onScroll={() => scroller.scrollTop < LOAD_MARGIN && loadOlder()}
      >
        <Show when={exhausted() && rows().length > 0}>
          <div class="start muted">Start of what Flek has for this conversation</div>
        </Show>
        <Show when={!loading() && rows().length === 0}>
          <div class="start muted">No messages yet. They appear here as they sync.</div>
        </Show>
        <For each={rows()}>
          {(m, i) => {
            const prev = () => rows()[i() - 1];
            const newDay = () => !prev() || dayKey(prev()!.ts) !== dayKey(m.ts);
            const grouped = () =>
              !newDay() &&
              !!prev() &&
              prev()!.user_id === m.user_id &&
              !m.subtype &&
              parseFloat(m.ts) - parseFloat(prev()!.ts) < GROUP_GAP;
            return (
              <>
                <Show when={newDay()}>
                  <div class="day">
                    <span>{dayLabel(m.ts)}</span>
                  </div>
                </Show>
                <article class="msg" classList={{ grouped: grouped() }}>
                  <Show when={!grouped()}>
                    <div class="meta">
                      <span class="author">{m.author ?? m.user_id ?? "unknown"}</span>
                      <time class="muted">{timeLabel(m.ts)}</time>
                      <Show when={m.hidden_by_limit}>
                        <span class="tag" title="Slack hides this message on the Free plan">
                          from archive
                        </span>
                      </Show>
                    </div>
                  </Show>
                  <div class="text">
                    {m.text}
                    <Show when={m.edited}>
                      <span class="muted edited"> (edited)</span>
                    </Show>
                  </div>
                  <Show when={m.reactions}>
                    <div class="reactions muted">{m.reactions}</div>
                  </Show>
                  <Show when={m.reply_count > 0}>
                    <div class="replies">
                      {m.reply_count} {m.reply_count === 1 ? "reply" : "replies"}
                    </div>
                  </Show>
                </article>
              </>
            );
          }}
        </For>
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

const timeLabel = (ts: string) =>
  tsDate(ts).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
