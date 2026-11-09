// One message, as both the timeline and the thread panel show it.
import { createSignal, For, Show } from "solid-js";
import { api, tsDate, type Message } from "./api";
import MessageMenu from "./MessageMenu";
import Avatar from "./Avatar";
import Emoji from "./EmojiView";
import Files from "./Files";
import { discard, retry } from "./outbox";
import { people } from "./people";
import { openProfile } from "./profile";
import Rich from "./Rich";
import { relativeTime } from "./time";
import Unfurls from "./Unfurls";

/// Messages from one author closer than this are grouped (seconds).
const GROUP_GAP = 5 * 60;

// "Last reply 5 minutes ago" ages while the window stays open.
const [now, setNow] = createSignal(new Date());
setInterval(() => setNow(new Date()), 60_000);

export const timeLabel = (ts: string) =>
  tsDate(ts).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });

export const author = (m: Message) =>
  (m.user_id && people()[m.user_id]) || m.author || m.user_id || "unknown";

export const repliesLabel = (n: number) => `${n} ${n === 1 ? "reply" : "replies"}`;

/// `:name: count` pairs (core's summary) as chips.
function Reactions(props: {
  m: Message;
  busy: boolean;
  onToggle: (name: string) => void;
  onPicker: (e: MouseEvent) => void;
}) {
  const items = () =>
    [...(props.m.reactions ?? "").matchAll(/:([^:\s]+(?:::skin-tone-\d)?): (\d+)/g)].map((m) => ({
      name: m[1],
      count: m[2],
    }));
  return (
    <div class="reactions">
      <For each={items()}>
        {(r) => (
          <button
            type="button"
            class="reaction"
            classList={{ mine: props.m.own_reactions?.includes(r.name) }}
            aria-pressed={props.m.own_reactions?.includes(r.name) ?? false}
            disabled={props.busy}
            title={`${props.m.own_reactions?.includes(r.name) ? "Remove" : "Add"} :${r.name}: reaction`}
            onClick={() => props.onToggle(r.name)}
          >
            <Emoji name={r.name} />
            <span>{r.count}</span>
          </button>
        )}
      </For>
      <button
        type="button"
        class="reaction reaction-add"
        title="Add reaction"
        aria-label="Add reaction"
        disabled={props.busy}
        onClick={props.onPicker}
      >
        ☺+
      </button>
    </div>
  );
}

/// Under a thread root: who replied, how many, and when (like Slack).
function ThreadBar(props: { m: Message; onOpen: () => void }) {
  const last = () => props.m.latest_reply;
  return (
    <button class="thread-bar" type="button" onClick={props.onOpen}>
      <Show when={props.m.repliers.length > 0}>
        <span class="thread-faces">
          <For each={props.m.repliers.slice(0, 4)}>
            {(id) => <Avatar id={id} name={people()[id] ?? id} size={20} />}
          </For>
        </span>
      </Show>
      <span class="thread-count">{repliesLabel(props.m.reply_count)}</span>
      <Show when={last()}>
        {(ts) => <span class="thread-last">Last reply {relativeTime(tsDate(ts()), now())}</span>}
      </Show>
      <span class="thread-view">View thread</span>
      <span class="thread-chevron" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

/// One message; `prev` (same day / same thread) decides grouping under one
/// author line. `onThread` shows the reply bar of a thread root.
export default function MessageRow(props: {
  m: Message;
  conversationId: string;
  prev?: Message;
  onThread?: (ts: string) => void;
  active?: boolean;
  /// Briefly highlighted (a search result jumped to).
  flash?: boolean;
}) {
  const m = props.m;
  const [menu, setMenu] = createSignal<{
    x: number;
    y: number;
    picker: boolean;
  } | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const openMenu = (e: MouseEvent, picker = false) => {
    if (m.pending) return;
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, picker });
  };
  const react = async (name: string) => {
    if (busy()) return;
    setMenu(null);
    setBusy(true);
    setError("");
    try {
      await api.setReaction(props.conversationId, m.ts, name, !m.own_reactions?.includes(name));
    } catch (e) {
      setError(`Reaction failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    setMenu(null);
    try {
      await navigator.clipboard.writeText(m.text);
    } catch {
      setError("Could not copy the message");
    }
  };
  const grouped = () =>
    !!props.prev &&
    props.prev.user_id === m.user_id &&
    !m.subtype &&
    parseFloat(m.ts) - parseFloat(props.prev.ts) < GROUP_GAP;
  return (
    <article
      class="msg"
      classList={{
        grouped: grouped(),
        active: props.active,
        flash: props.flash,
        sending: m.pending === "sending",
        failed: m.pending === "failed",
      }}
      data-ts={m.ts}
      onContextMenu={(e) => openMenu(e)}
      onKeyDown={(e) => {
        if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
          e.preventDefault();
          const rect = e.currentTarget.getBoundingClientRect();
          if (!m.pending) setMenu({ x: rect.left + 48, y: rect.top + 24, picker: false });
        }
      }}
    >
      <Show when={!m.pending}>
        <div class="message-actions">
          <button
            type="button"
            title="Add reaction"
            aria-label="Add reaction"
            disabled={busy()}
            onClick={(e) => openMenu(e, true)}
          >
            ☺+
          </button>
          <Show when={props.onThread}>
            <button
              type="button"
              title="Reply in thread"
              aria-label="Reply in thread"
              onClick={() => props.onThread?.(m.thread_ts ?? m.ts)}
            >
              ↩
            </button>
          </Show>
          <button
            type="button"
            title="More actions"
            aria-label="More message actions"
            onClick={(e) => openMenu(e)}
          >
            ⋯
          </button>
        </div>
      </Show>
      <Show when={menu()}>
        {(pos) => (
          <MessageMenu
            x={pos().x}
            y={pos().y}
            picker={pos().picker}
            own={m.own_reactions ?? []}
            onReaction={react}
            onClose={() => setMenu(null)}
            onCopy={copy}
            onThread={props.onThread ? () => props.onThread?.(m.thread_ts ?? m.ts) : undefined}
          />
        )}
      </Show>
      <div class="gutter">
        <Show when={!grouped()} fallback={<time class="hover-time">{timeLabel(m.ts)}</time>}>
          <Show when={m.user_id} fallback={<Avatar id={m.user_id} name={author(m)} />}>
            {(id) => (
              <button
                class="avatar-button"
                type="button"
                title={`View ${author(m)}'s profile`}
                onClick={() => openProfile(id())}
              >
                <Avatar id={id()} name={author(m)} />
              </button>
            )}
          </Show>
        </Show>
      </div>
      <div class="body">
        <Show when={!grouped()}>
          <div class="meta">
            <Show when={m.user_id} fallback={<span class="author">{author(m)}</span>}>
              {(id) => (
                <button
                  class="author author-button"
                  type="button"
                  onClick={() => openProfile(id())}
                >
                  {author(m)}
                </button>
              )}
            </Show>
            <time class="muted">{timeLabel(m.ts)}</time>
            <Show when={m.hidden_by_limit}>
              <span class="tag" title="Slack hides this message on the Free plan">
                from archive
              </span>
            </Show>
          </div>
        </Show>
        <Show when={m.text.trim()}>
          <Rich text={m.text} />
        </Show>
        <Show when={m.edited}>
          <span class="muted edited">(edited)</span>
        </Show>
        <Show when={m.files.length > 0}>
          <Files files={m.files} from={{ user_id: m.user_id, name: author(m), ts: m.ts }} />
        </Show>
        <Show when={m.unfurls.length > 0}>
          <Unfurls items={m.unfurls} />
        </Show>
        <Show when={m.reactions && !m.pending}>
          <Reactions m={m} busy={busy()} onToggle={react} onPicker={(e) => openMenu(e, true)} />
        </Show>
        <Show when={error()}>
          <div class="reaction-error" role="alert">
            {error()}{" "}
            <button type="button" aria-label="Dismiss error" onClick={() => setError("")}>
              ×
            </button>
          </div>
        </Show>
        <Show when={m.pending === "failed" && m.client_msg_id}>
          {(id) => (
            <div class="send-failed" role="alert" title={m.error}>
              <span>Not sent</span>
              <span aria-hidden="true">·</span>
              <button type="button" onClick={() => retry(id())}>
                Retry
              </button>
              <span aria-hidden="true">·</span>
              <button type="button" onClick={() => discard(id())}>
                Discard
              </button>
              <Show when={m.error}>
                <span class="send-reason">{m.error}</span>
              </Show>
            </div>
          )}
        </Show>
        <Show when={props.onThread && m.reply_count > 0}>
          <ThreadBar m={m} onOpen={() => props.onThread?.(m.ts)} />
        </Show>
      </div>
    </article>
  );
}
