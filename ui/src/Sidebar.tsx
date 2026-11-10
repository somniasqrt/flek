import { createSignal, For, Show, type JSX } from "solid-js";
import type { Conversation } from "./api";
import Avatar from "./Avatar";
import CustomStatus from "./CustomStatus";
import { presenceOf, statuses } from "./presence-store";
import { Caret, Hash, Lock, People, Search } from "./icons";

/// Channels by name, then direct messages by latest activity.
export default function Sidebar(props: {
  team: string;
  conversations: Conversation[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const [query, setQuery] = createSignal("");
  const live = () => {
    const q = query().trim().toLowerCase();
    return props.conversations.filter(
      (c) => !c.is_archived && (!q || c.label.toLowerCase().includes(q)),
    );
  };
  const channels = () =>
    live()
      .filter((c) => c.kind === "public_channel" || c.kind === "private_channel")
      .sort((a, b) => a.label.localeCompare(b.label));
  const direct = () =>
    live()
      .filter((c) => c.kind === "im" || c.kind === "mpim")
      .sort((a, b) => (b.latest_ts ?? "").localeCompare(a.latest_ts ?? ""));

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") setQuery("");
    if (e.key === "Enter") {
      const first = channels()[0] ?? direct()[0];
      if (first) {
        props.onSelect(first.id);
        setQuery("");
      }
    }
  };

  const Item = (p: { c: Conversation }) => (
    <li>
      <button
        class="conv"
        classList={{ active: p.c.id === props.selected, unread: (p.c.unread ?? 0) > 0 }}
        onClick={() => props.onSelect(p.c.id)}
        title={p.c.topic ?? p.c.label}
      >
        <span class="conv-icon">
          {p.c.kind === "im" ? (
            <Avatar
              id={p.c.dm_user_id}
              name={p.c.label}
              size={20}
              presence={presenceOf(p.c.dm_user_id)}
            />
          ) : p.c.kind === "public_channel" ? (
            <Hash />
          ) : p.c.kind === "private_channel" ? (
            <Lock />
          ) : (
            <People />
          )}
        </span>
        <span class="name">
          {p.c.label}
          <Show when={p.c.kind === "im" && p.c.dm_user_id}>
            {(id) => <CustomStatus status={statuses()[id()]} />}
          </Show>
        </span>
        <Show when={(p.c.unread ?? 0) > 0 && (p.c.kind === "im" || p.c.kind === "mpim")}>
          <span class="badge">{p.c.unread}</span>
        </Show>
      </button>
    </li>
  );

  return (
    <nav class="sidebar">
      <header>
        <div class="team">{props.team}</div>
      </header>
      <label class="find">
        <Search size={14} />
        <input
          type="search"
          placeholder="Find a conversation…"
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={onKey}
        />
      </label>
      <div class="sections">
        <Section title="Channels" items={channels()} item={Item} />
        <Section title="Direct messages" items={direct()} item={Item} />
        <Show when={query() && channels().length + direct().length === 0}>
          <p class="nothing">No conversations match</p>
        </Show>
      </div>
    </nav>
  );
}

function Section(props: {
  title: string;
  items: Conversation[];
  item: (p: { c: Conversation }) => JSX.Element;
}) {
  const [open, setOpen] = createSignal(true);
  // A collapsed section still shows what needs attention, like Slack's.
  const shown = () =>
    open() ? props.items : props.items.filter((c) => (c.unread ?? 0) > 0);
  return (
    <Show when={props.items.length > 0}>
      <button class="section" classList={{ open: open() }} onClick={() => setOpen(!open())}>
        <span class="caret">
          <Caret size={14} />
        </span>
        {props.title}
      </button>
      <ul>
        <For each={shown()}>{(c) => props.item({ c })}</For>
      </ul>
    </Show>
  );
}
