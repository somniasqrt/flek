import { createSignal, For, Show } from "solid-js";
import { t, locale } from "./i18n";
import { settings } from "./settings";
import { sortDms } from "./settings-model";
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
  mutedIds: string[];
  me: string;
  selected: string | null;
  onSelect: (id: string) => void;
  onContext: (target: ConversationMenuTarget) => void;
  onCompose: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [unread, setUnread] = createSignal(false);
  const shown = () =>
    sortDms(
      props.rows.filter(
        (r) =>
          r.label.toLowerCase().includes(query().trim().toLowerCase()) &&
          (!unread() || (r.unread ?? 0) > 0),
      ),
      settings().dm_sort,
      locale(),
    );
  const Row = (p: { row: DirectMessage }) => {
    const r = p.row;
    return (
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
            presence={settings().presence_dots ? presenceOf(r.partner_id) : undefined}
          />
        </Show>
        <span class="dm-row-body">
          <span class="dm-row-head">
            <span class="dm-name">
              {r.partner_id === props.me ? t("{name} (you)", { name: r.label }) : r.label}
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
    );
  };
  return (
    <nav class="rail-list dm-list" aria-label={t("Direct messages")}>
      <header class="rail-list-header">
        <h2>{t("Direct messages")}</h2>
        <button aria-label={t("New message")} class="plain-icon" onClick={props.onCompose}>
          <Plus size={20} />
        </button>
      </header>
      <input
        class="view-search"
        type="search"
        aria-label={t("Find a DM")}
        placeholder={t("Find a DM…")}
        value={query()}
        onInput={(e) => setQuery(e.currentTarget.value)}
      />
      <label class="view-toggle">
        <input
          type="checkbox"
          checked={unread()}
          onChange={(e) => setUnread(e.currentTarget.checked)}
        />
        {t("Unread messages")}
      </label>
      <div class="rail-list-scroll">
        <For each={shown().filter((r) => !props.mutedIds.includes(r.id))}>
          {(r) => <Row row={r} />}
        </For>
        <Show when={settings().show_muted && shown().some((r) => props.mutedIds.includes(r.id))}>
          <details class="dm-muted-group">
            <summary>{t("Muted")}</summary>
            <For each={shown().filter((r) => props.mutedIds.includes(r.id))}>
              {(r) => <Row row={r} />}
            </For>
          </details>
        </Show>
        <Show when={shown().length === 0}>
          <p class="view-empty muted">{t("No direct messages match.")}</p>
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
