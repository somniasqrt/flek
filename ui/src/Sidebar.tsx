import { For, Show } from "solid-js";
import type { Conversation } from "./api";
import Avatar from "./Avatar";

/// Channels by name, then direct messages by latest activity.
export default function Sidebar(props: {
  team: string;
  user: string;
  conversations: Conversation[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const live = () => props.conversations.filter((c) => !c.is_archived);
  const channels = () =>
    live()
      .filter((c) => c.kind === "public_channel" || c.kind === "private_channel")
      .sort((a, b) => a.label.localeCompare(b.label));
  const direct = () =>
    live()
      .filter((c) => c.kind === "im" || c.kind === "mpim")
      .sort((a, b) => (b.latest_ts ?? "").localeCompare(a.latest_ts ?? ""));

  const Item = (p: { c: Conversation }) => (
    <li>
      <button
        class="conv"
        classList={{ active: p.c.id === props.selected, unread: (p.c.unread ?? 0) > 0 }}
        onClick={() => props.onSelect(p.c.id)}
        title={p.c.topic ?? p.c.label}
      >
        <Show
          when={p.c.kind === "im"}
          fallback={
            <span class="prefix">
              {p.c.kind === "public_channel" ? "#" : p.c.kind === "private_channel" ? "🔒" : "●"}
            </span>
          }
        >
          <Avatar id={p.c.dm_user_id} name={p.c.label} size={20} />
        </Show>
        <span class="name">{p.c.label}</span>
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
        <div class="me muted">{props.user}</div>
      </header>
      <div class="sections">
        <h3>Channels</h3>
        <ul>
          <For each={channels()}>{(c) => <Item c={c} />}</For>
        </ul>
        <h3>Direct messages</h3>
        <ul>
          <For each={direct()}>{(c) => <Item c={c} />}</For>
        </ul>
      </div>
    </nav>
  );
}
