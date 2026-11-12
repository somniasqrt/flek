import { createEffect, createMemo, createSignal, For, on, Show, type JSX } from "solid-js";
import { t, locale } from "../i18n/index";
import { settings } from "../stores/settings";
import { sortDms } from "../models/settings-model";
import type { Conversation } from "../api";
import { isContextKey, menuTarget, type ConversationMenuTarget } from "../components/ConversationMenu";
import { hasUnread } from "../models/conversation-model";
import Avatar from "../components/Avatar";
import CustomStatus from "../components/CustomStatus";
import { presenceOf, statuses } from "../stores/presence-store";
import { Caret, Hash, Lock, People, Search } from "../components/icons";

/// Channels by name, then direct messages by latest activity.
export default function Sidebar(props: {
  team: string;
  conversations: Conversation[];
  selected: string | null;
  onSelect: (id: string) => void;
  onContext: (target: ConversationMenuTarget) => void;
  onChannelsMenu: (position: { x: number; y: number }) => void;
}) {
  const [query, setQuery] = createSignal("");
  createEffect(on(() => props.selected, () => setQuery(""), { defer: true }));
  const live = () => {
    const q = query().trim().toLowerCase();
    return props.conversations.filter(
      (c) => !c.is_archived && (!q || c.label.toLowerCase().includes(q)),
    );
  };
  const normal = () => live().filter((c) => !c.starred && c.notification !== "muted");
  const channels = () =>
    normal()
      .filter((c) => c.kind === "public_channel" || c.kind === "private_channel")
      .sort((a, b) => a.label.localeCompare(b.label));
  const direct = () =>
    sortDms(
      normal().filter((c) => c.kind === "im" || c.kind === "mpim"),
      settings().dm_sort,
      locale(),
    );

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") setQuery("");
    if (e.key === "Enter") {
      const first = live().find((c) => c.starred) ?? channels()[0] ?? direct()[0] ?? live()[0];
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
        data-conversation-id={p.c.id}
        aria-current={p.c.id === props.selected ? "page" : undefined}
        classList={{
          active: p.c.id === props.selected,
          unread: hasUnread(p.c),
          muted: p.c.notification === "muted",
        }}
        onContextMenu={(e) => props.onContext(menuTarget(e, p.c.id))}
        onKeyDown={(e) => {
          if (isContextKey(e)) props.onContext(menuTarget(e, p.c.id));
        }}
        onClick={() => props.onSelect(p.c.id)}
        title={p.c.topic ?? p.c.label}
      >
        <span class="conv-icon">
          {p.c.kind === "im" ? (
            <Avatar
              id={p.c.dm_user_id}
              name={p.c.label}
              size={20}
              presence={settings().presence_dots ? presenceOf(p.c.dm_user_id) : undefined}
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
        <Show when={hasUnread(p.c) && (p.c.kind === "im" || p.c.kind === "mpim")}>
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
          placeholder={t("Find a conversation…")}
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={onKey}
        />
      </label>
      <div class="sections">
        <Section
          title={t("Starred")}
          items={live().filter((c) => c.starred && c.notification !== "muted")}
          selected={props.selected}
          item={Item}
        />
        <Section
          title={t("Channels")}
          items={channels()}
          selected={props.selected}
          item={Item}
          onMenu={props.onChannelsMenu}
        />
        <Section title={t("Direct messages")} items={direct()} selected={props.selected} item={Item} />
        <Section
          title={t("Muted")}
          items={settings().show_muted ? live().filter((c) => c.notification === "muted") : []}
          selected={props.selected}
          item={Item}
          collapsed
        />
        <Show when={query() && live().length === 0}>
          <p class="nothing">{t("No conversations match")}</p>
        </Show>
      </div>
    </nav>
  );
}

function Section(props: {
  title: string;
  selected: string | null;
  collapsed?: boolean;
  onMenu?: (position: { x: number; y: number }) => void;
  items: Conversation[];
  item: (p: { c: Conversation }) => JSX.Element;
}) {
  const [open, setOpen] = createSignal(!props.collapsed);
  const containsSelected = createMemo(() => props.items.some((c) => c.id === props.selected));
  createEffect(() => { if (containsSelected()) setOpen(true); });
  // A collapsed section still shows what needs attention, like Slack's.
  const shown = () => (open() ? props.items : props.items.filter(hasUnread));
  return (
    <Show when={props.items.length > 0 || props.onMenu}>
      <div
        class="section-heading"
        onContextMenu={(e) => {
          if (!props.onMenu) return;
          e.preventDefault();
          props.onMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <button
          class="section"
          classList={{ open: open() }}
          aria-expanded={open()}
          onClick={() => setOpen(!open())}
          onKeyDown={(e) => {
            if (!props.onMenu || !isContextKey(e)) return;
            e.preventDefault();
            const rect = e.currentTarget.getBoundingClientRect();
            props.onMenu({ x: rect.left, y: rect.bottom });
          }}
        >
          <span class="caret">
            <Caret size={14} />
          </span>
          {props.title}
        </button>
        <Show when={props.onMenu}>
          <button
            class="section-menu plain-icon"
            aria-label={t("Channels menu")}
            aria-haspopup="menu"
            onClick={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              props.onMenu?.({ x: rect.left, y: rect.bottom });
            }}
          >
            ⋮
          </button>
        </Show>
      </div>
      <ul>
        <For each={shown()}>{(c) => props.item({ c })}</For>
      </ul>
    </Show>
  );
}
