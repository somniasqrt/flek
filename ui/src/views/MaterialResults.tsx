import { createEffect, createSignal, For, on, onCleanup, Show } from "solid-js";
import { api, tsDate, type Hit, type Material, type MaterialSort } from "../api";
import { errorMessage, locale, t } from "../i18n";

const PAGE = 100;

/** The catalogue uses the same query as messages; pagination stays in core. */
export default function MaterialResults(props: {
  query: string;
  revision: number;
  onOpen: (hit: Hit) => void;
}) {
  const [sort, setSort] = createSignal<MaterialSort>("newest");
  const [items, setItems] = createSignal<Material[]>([]);
  const [cursor, setCursor] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let generation = 0;
  onCleanup(() => ++generation);

  const load = async (append: boolean) => {
    const n = ++generation;
    setBusy(true);
    setError(null);
    if (!append) {
      setItems([]);
      setCursor(null);
    }
    try {
      const page = await api.searchMaterials(props.query, sort(), PAGE, append ? cursor() : null);
      if (n !== generation) return;
      setItems((old) => append ? [...old, ...page.items] : page.items);
      setCursor(page.next_cursor);
    } catch (e) {
      if (n === generation) setError(String(e));
    } finally {
      if (n === generation) setBusy(false);
    }
  };
  createEffect(on([() => props.query, sort, () => props.revision], () => { void load(false); }));

  return (
    <div class="scroller material-results">
      <div class="materials-toolbar">
        <p class="muted small">
          {t("Links and files from your local archive. Use in:, from:, after: and before: to filter.")}
        </p>
        <label>
          {t("Sort")}{" "}
          <select value={sort()} onChange={(e) => setSort(e.currentTarget.value as MaterialSort)}>
            <option value="newest">{t("Newest")}</option>
            <option value="oldest">{t("Oldest")}</option>
            <option value="name">{t("Name")}</option>
          </select>
        </label>
      </div>
      <Show when={error()}>
        {(e) => <p class="material-error" role="alert">{errorMessage(e())}</p>}
      </Show>
      <Show when={items().length > 0}>
        <table class="materials-table" aria-label={t("Links and files")} aria-busy={busy()}>
          <thead>
            <tr>
              <th scope="col">{t("Material")}</th>
              <th scope="col">{t("Conversation")}</th>
              <th scope="col">{t("Author")}</th>
              <th scope="col">{t("Date")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={items()}>
              {(material) => (
                <tr>
                  <td>
                    <span class="muted small">{material.kind === "link" ? t("Link") : t("File")}</span>
                    <button
                      class="material-title"
                      title={t("Jump to message")}
                      onClick={() => props.onOpen(material)}
                    >
                      {material.title}
                    </button>
                    <Show when={material.kind === "link" && material.url && material.url !== material.title}>
                      <span class="material-url muted small">{material.url}</span>
                    </Show>
                    <Show when={material.thread_ts && material.thread_ts !== material.ts}>
                      <span class="muted small">{t("In a thread")}</span>
                    </Show>
                  </td>
                  <td>{material.conversation}</td>
                  <td>{material.author ?? t("unknown")}</td>
                  <td>
                    <time dateTime={tsDate(material.ts).toISOString()}>
                      {tsDate(material.ts).toLocaleDateString(locale())}
                    </time>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>
      <Show when={!busy() && !error() && items().length === 0}>
        <div class="no-results">
          <h3>{t("No materials found")}</h3>
          <p class="muted">{t("Try fewer words or other filters.")}</p>
        </div>
      </Show>
      <Show when={busy()}><div class="spinner" /></Show>
      <Show when={cursor()}>
        <button class="materials-more" disabled={busy()} onClick={() => { void load(true); }}>
          {t("Load more")}
        </button>
      </Show>
    </div>
  );
}
