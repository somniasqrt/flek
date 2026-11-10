import { createSignal, For, Show } from "solid-js";
import { tsDate, type DirectMessage } from "./api";
import { isContextKey, menuTarget, type ConversationMenuTarget } from "./ConversationMenu";
import Avatar from "./Avatar";
import { emoji } from "./emoji";
import { Plus } from "./icons";
import { people } from "./people";
import { presenceNow, presenceOf } from "./presence-store";
import { messagePreview } from "./rail-model";
import { listTime } from "./time";

export default function DirectMessages(props: {
  rows: DirectMessage[];
  me: string;
  selected: string | null;
  onSelect: (id: string) => void;
  onContext: (target: ConversationMenuTarget) => void;
  onCompose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [unread, setUnread] = createSignal(false);
  const shown = () =>
    props.rows.filter(
      (r) =>
        r.label.toLowerCase().includes(query().trim().toLowerCase()) &&
        (!unread() || (r.unread ?? 0) > 0),
    );
  return (
    <nav class="rail-list dm-list" aria-label="Direct messages">
      <header class="rail-list-header">
        <h2>Direct messages</h2>
        <button aria-label="New message" class="plain-icon" onClick={props.onCompose}>
          <Plus size={20} />
        </button>
      </header>
      <input
        class="view-search"
        type="search"
        aria-label="Find a DM"
        placeholder="Find a DM…"
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
      />
      <label class="view-toggle">
        <input
          type="checkbox"
          checked={unread()}
          onChange={(e) => setUnread(e.currentTarget.checked)}
        />
        Unread messages
      </label>
      <div class="rail-list-scroll">
        <For each={shown()}>
          {(r) => (
            <button
              class="dm-row"
              classList={{
                active: props.selected === r.id,
                unread: (r.unread ?? 0) > 0,
              }}
              onContextMenu={(e) => props.onContext(menuTarget(e, r.id))}
              onKeyDown={(e) => {
                if (isContextKey(e)) props.onContext(menuTarget(e, r.id));
              }}
              onClick={() => props.onSelect(r.id)}
            >
              <Show
                when={r.kind === "im"}
                fallback={<GroupAvatars members={r.members} label={r.label} />}
              >
                <Avatar
                  id={r.partner_id}
                  name={r.label}
                  size={40}
                  presence={presenceOf(r.partner_id)}
                />
              </Show>
              <span class="dm-row-body">
                <span class="dm-row-head">
                  <span class="dm-name">
                    {r.label}
                    {r.partner_id === props.me ? " (you)" : ""}
                  </span>
                  <Show when={r.ts}>
                    {(ts) => <time>{listTime(tsDate(ts()), new Date(presenceNow() * 1000))}</time>}
                  </Show>
                </span>
                <span class="dm-row-preview">
                  <span>{messagePreview(r, props.me, people(), emoji)}</span>
                  <Show when={(r.unread ?? 0) > 0}>
                    <span class="badge">{r.unread}</span>
                  </Show>
                </span>
              </span>
            </button>
          )}
        </For>
        <Show when={shown().length === 0}>
          <p class="view-empty muted">No direct messages match.</p>
        </Show>
      </div>
    </nav>
  );
}

export function GroupAvatars(props: { members: string[]; label: string }) {
  return (
    <span class="group-avatars">
      <For each={props.members.slice(0, 2)}>
        {(id) => <Avatar id={id} name={people()[id] ?? id} size={28} />}
      </For>
      <Show when={props.members.length === 0}>
        <Avatar id={null} name={props.label} size={40} />
      </Show>
    </span>
  );
}
