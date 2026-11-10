import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, tsDate, type ActivityItem, type Conversation } from "./api";
import Avatar from "./Avatar";
import { emoji } from "./emoji";
import Emoji from "./EmojiView";
import { people } from "./people";
import { presenceNow } from "./presence-store";
import { messagePreview } from "./rail-model";
import type { Jump } from "./Search";
import { activityDay, listTime } from "./time";

const tabs = [
  ["all", "All"],
  ["dm", "DMs"],
  ["mention", "Mentions"],
  ["thread", "Threads"],
] as const;

export default function ActivityView(props: {
  revision: number;
  conversations: Conversation[];
  me: string;
  onOpen: (jump: Jump) => void;
}) {
  const [tab, setTab] = createSignal("all");
  const [unread, setUnread] = createSignal(false);
  const [items, setItems] = createSignal<ActivityItem[]>([]);
  const [more, setMore] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  let generation = 0;
  let requested = 50;
  onCleanup(() => generation++);
  const load = async (append = false) => {
    const gen = ++generation;
    const offset = append ? items().length : 0;
    if (append) requested = offset + 50;
    setBusy(true);
    setError("");
    try {
      const target = append ? 50 : requested;
      const collected: ActivityItem[] = [];
      let hasMore = true;
      while (hasMore && collected.length < target) {
        const page = await api.activity(
          tab(),
          unread(),
          Math.min(500, target - collected.length),
          offset + collected.length,
        );
        if (gen !== generation) return;
        collected.push(...page.items);
        hasMore = page.has_more && page.items.length > 0;
      }
      setItems(append ? [...items(), ...collected] : collected);
      setMore(hasMore);
    } catch (e) {
      if (gen === generation) setError(String(e));
    } finally {
      if (gen === generation) setBusy(false);
    }
  };
  createEffect(
    on([tab, unread], () => {
      requested = 50;
      setItems([]);
      load();
    }),
  );
  createEffect(
    on(
      () => props.revision,
      () => load(),
      { defer: true },
    ),
  );
  const days = createMemo(() => {
    const groups = new Map<string, ActivityItem[]>();
    for (const item of items()) {
      const key = tsDate(item.event_ts).toDateString();
      groups.set(key, [...(groups.get(key) ?? []), item]);
    }
    return [...groups.values()];
  });
  const context = (item: ActivityItem) => {
    const c = props.conversations.find((c) => c.id === item.conversation_id);
    const label = c?.label ?? item.conversation_id;
    if (item.kind === "dm") return "Direct message";
    const where = c?.kind === "im" || c?.kind === "mpim" ? label : `#${label}`;
    return item.kind === "thread" ? `Thread in ${where}` : where;
  };
  return (
    <nav class="rail-list activity-list" aria-label="Activity">
      <header class="rail-list-header">
        <h2>Activity</h2>
      </header>
      <div class="activity-tabs" role="tablist" aria-label="Activity kind">
        <For each={tabs}>
          {([id, label]) => (
            <button role="tab" aria-selected={tab() === id} onClick={() => setTab(id)}>
              {label}
            </button>
          )}
        </For>
      </div>
      <label class="view-toggle">
        <input
          type="checkbox"
          checked={unread()}
          onChange={(e) => setUnread(e.currentTarget.checked)}
        />
        Unread
      </label>
      <div class="rail-list-scroll">
        <For each={days()}>
          {(group) => (
            <section>
              <h3 class="activity-day">
                {activityDay(tsDate(group[0].event_ts), new Date(presenceNow() * 1000))}
              </h3>
              <For each={group}>
                {(item) => (
                  <button
                    class="activity-row"
                    classList={{ unread: item.unread }}
                    onClick={() => props.onOpen(item)}
                  >
                    <Avatar id={item.user_id} name={people()[item.user_id ?? ""] ?? "Unknown"} />
                    <span class="activity-body">
                      <span class="dm-row-head">
                        <span class="dm-name">{people()[item.user_id ?? ""] ?? "Unknown"}</span>
                        <time>
                          {listTime(tsDate(item.event_ts), new Date(presenceNow() * 1000))}
                        </time>
                      </span>
                      <span class="activity-context">{context(item)}</span>
                      <span class="activity-preview">
                        <Show when={item.kind === "reaction"}>
                          Reacted <Emoji name={item.reaction ?? "eyes"} inText /> to:{" "}
                        </Show>
                        {messagePreview({ ...item, user_id: null }, props.me, people(), emoji)}
                      </span>
                      <Show when={item.reaction}>
                        {(name) => (
                          <span class="activity-reaction">
                            <Emoji name={name()} /> 1
                          </span>
                        )}
                      </Show>
                    </span>
                    <Show when={item.unread}>
                      <span class="activity-unread" aria-label="Unread">
                        {item.kind === "dm" ? item.unread_count : "•"}
                      </span>
                    </Show>
                  </button>
                )}
              </For>
            </section>
          )}
        </For>
        <Show when={error()}>
          <p class="error view-empty" role="alert">
            {error()}
          </p>
        </Show>
        <Show when={!busy() && !error() && items().length === 0}>
          <p class="view-empty muted">You're all caught up.</p>
        </Show>
        <Show when={more()}>
          <button class="load-more" disabled={busy()} onClick={() => load(true)}>
            Load more
          </button>
        </Show>
      </div>
    </nav>
  );
}
