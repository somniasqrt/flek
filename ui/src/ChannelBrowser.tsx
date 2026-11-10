import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import { api, type ChannelDirectory } from "./api";
import { errorMessage, locale, plural, t } from "./i18n";
import { filterChannels, type Filters } from "./channel-browser-model";
import { Hash, Lock } from "./icons";

export default function ChannelBrowser(props: {
  onOpen: (id: string, preview: boolean) => Promise<void>;
  onJoin: (id: string) => Promise<void>;
  onCreate: () => void;
  onClose: () => void;
}) {
  const [directory, setDirectory] = createSignal<ChannelDirectory | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [acting, setActing] = createSignal<string | null>(null);
  const [error, setError] = createSignal("");
  const [filters, setFilters] = createSignal<Filters>({
    query: "", membership: "all", visibility: "all", archived: false, sort: "name",
  });
  const rows = createMemo(() => filterChannels(directory()?.channels ?? [], filters(), locale()));
  const update = <K extends keyof Filters>(key: K, value: Filters[K]) =>
    setFilters((old) => ({ ...old, [key]: value }));
  const load = async (force: boolean) => {
    setBusy(true);
    setError("");
    try {
      // Show cached rows before waiting on a network refresh.
      setDirectory(await api.channelDirectory());
      await api.channelDirectory(true, force);
      setDirectory(await api.channelDirectory());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  onMount(() => void load(false));
  const action = async (id: string, task: () => Promise<void>) => {
    if (acting()) return;
    setActing(id);
    setError("");
    try {
      await task();
    } catch (e) {
      setError(String(e));
    } finally {
      setActing(null);
    }
  };
  return (
    <div class="channel-browser">
      <header class="conv-header">
        <h2>{t("Channels")}</h2>
        <button onClick={() => load(true)} disabled={busy()}>
          {busy() ? t("Loading…") : t("Refresh channels")}
        </button>
        <button onClick={props.onCreate}>{t("Create a channel")}</button>
        <button class="plain-icon" aria-label={t("Close channel browser")} onClick={props.onClose}>
          ×
        </button>
      </header>
      <div class="channel-filters">
        <input
          type="search"
          autofocus
          aria-label={t("Search channels")}
          placeholder={t("Search by name, topic or description")}
          value={filters().query}
          onInput={(e) => update("query", e.currentTarget.value)}
        />
        <label>
          {t("Membership")}
          <select
            value={filters().membership}
            onChange={(e) => update("membership", e.currentTarget.value as Filters["membership"])}
          >
            <option value="all">{t("All channels")}</option>
            <option value="joined">{t("Joined")}</option>
            <option value="not_joined">{t("Not joined")}</option>
          </select>
        </label>
        <label>
          {t("Visibility")}
          <select
            value={filters().visibility}
            onChange={(e) => update("visibility", e.currentTarget.value as Filters["visibility"])}
          >
            <option value="all">{t("All")}</option>
            <option value="public">{t("Public")}</option>
            <option value="private">{t("Private")}</option>
          </select>
        </label>
        <label>
          {t("Sort channels")}
          <select
            value={filters().sort}
            onChange={(e) => update("sort", e.currentTarget.value as Filters["sort"])}
          >
            <option value="name">{t("Name")}</option>
            <option value="members">{t("Most members")}</option>
            <option value="newest">{t("Newest")}</option>
          </select>
        </label>
        <label class="channel-checkbox">
          <input
            type="checkbox"
            checked={filters().archived}
            onChange={(e) => update("archived", e.currentTarget.checked)}
          />
          {t("Include archived")}
        </label>
      </div>
      <Show when={error()}>
        <p class="error channel-notice" role="alert">{errorMessage(error())}</p>
      </Show>
      <Show when={directory()?.truncated}>
        <p class="channel-notice" role="status">
          {t("Showing the first 20 pages returned by Slack. Some channels may be missing.")}
        </p>
      </Show>
      <div class="channel-directory-rows" aria-busy={busy()}>
        <For each={rows()}>
          {(row) => (
            <article class="channel-directory-row">
              <div class="channel-directory-icon">
                <Show when={row.is_private} fallback={<Hash />}><Lock /></Show>
              </div>
              <div class="channel-directory-copy">
                <h3>{row.name}</h3>
                <p class="muted">
                  {row.num_members === null ? t("Members not cached") : plural("members", row.num_members)}
                </p>
                <p>{row.topic || row.purpose || t("No description")}</p>
                <Show when={row.is_archived}><span class="muted">{t("Archived")}</span></Show>
              </div>
              <Show when={row.is_member}>
                <span class="channel-joined">{t("✓ Joined")}</span>
              </Show>
              <div class="channel-row-actions">
                <button
                  disabled={!!acting()}
                  onClick={() => action(row.id, () => props.onOpen(row.id, !row.is_member))}
                >
                  {t("View")}
                </button>
                <Show when={row.is_member} fallback={
                  <button
                    disabled={!!acting() || row.is_archived || row.is_private}
                    onClick={() => action(row.id, () => props.onJoin(row.id))}
                  >
                    {acting() === row.id ? t("Loading…") : t("Join")}
                  </button>
                }>
                  <button
                    disabled={!!acting()}
                    onClick={() => action(row.id, () => props.onOpen(row.id, false))}
                  >
                    {t("Open")}
                  </button>
                </Show>
              </div>
            </article>
          )}
        </For>
        <Show when={!busy() && rows().length === 0}>
          <p class="view-empty muted">{t("No channels match")}</p>
        </Show>
      </div>
    </div>
  );
}
