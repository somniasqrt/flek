// One message, as both the timeline and the thread panel show it.
import { createSignal, For, Show } from "solid-js";
import { tsDate, type Message } from "./api";
import Avatar from "./Avatar";
import { emoji } from "./emoji";
import Files from "./Files";
import { people } from "./people";
import Rich from "./Rich";
import { relativeTime } from "./time";
import Unfurls from "./Unfurls";

/// Messages from one author closer than this are grouped (seconds).
const GROUP_GAP = 5 * 60;

// "Last reply 5 minutes ago" ages while the window stays open.
const [now, setNow] = createSignal(new Date());
setInterval(() => setNow(new Date()), 60_000);

export const timeLabel = (ts: string) =>
  tsDate(ts).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

export const author = (m: Message) =>
  (m.user_id && people()[m.user_id]) || m.author || m.user_id || "unknown";

export const repliesLabel = (n: number) => `${n} ${n === 1 ? "reply" : "replies"}`;

/// `:name: count` pairs (core's summary) as chips.
function Reactions(props: { summary: string }) {
  const items = () =>
    [...props.summary.matchAll(/:([^:\s]+(?:::skin-tone-\d)?): (\d+)/g)].map((m) => ({
      name: m[1],
      count: m[2],
    }));
  return (
    <div class="reactions">
      <For each={items()}>
        {(r) => (
          <span class="reaction" title={`:${r.name}:`}>
            <span class="emoji">{emoji(r.name) ?? `:${r.name}:`}</span>
            <span>{r.count}</span>
          </span>
        )}
      </For>
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
        {(ts) => (
          <span class="thread-last">Last reply {relativeTime(tsDate(ts()), now())}</span>
        )}
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
  prev?: Message;
  onThread?: (ts: string) => void;
  active?: boolean;
}) {
  const m = props.m;
  const grouped = () =>
    !!props.prev &&
    props.prev.user_id === m.user_id &&
    !m.subtype &&
    parseFloat(m.ts) - parseFloat(props.prev.ts) < GROUP_GAP;
  return (
    <article class="msg" classList={{ grouped: grouped(), active: props.active }}>
      <div class="gutter">
        <Show when={!grouped()} fallback={<time class="hover-time">{timeLabel(m.ts)}</time>}>
          <Avatar id={m.user_id} name={author(m)} />
        </Show>
      </div>
      <div class="body">
        <Show when={!grouped()}>
          <div class="meta">
            <span class="author">{author(m)}</span>
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
        <Show when={m.reactions}>{(r) => <Reactions summary={r()} />}</Show>
        <Show when={props.onThread && m.reply_count > 0}>
          <ThreadBar m={m} onOpen={() => props.onThread?.(m.ts)} />
        </Show>
      </div>
    </article>
  );
}
