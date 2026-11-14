import { fileMenu } from "../components/file-menu";
import { downloadAttachment, downloading } from "../components/Download";
import SvgPreview from "../components/SvgPreview";
import { settings } from "../stores/settings";
import FileSafety, { fileActionLabel } from "../components/FileSafety";
import { isRasterImage, fileName } from "../models/file-presentation";
import { createEffect, createSignal, For, on, onCleanup, Show } from "solid-js";
import { t, errorMessage, locale } from "../i18n/index";
import { api, fileUrl, tsDate, type Conversation, type StoredFile } from "../api";
import { Document } from "../components/icons";
import { people } from "../stores/people";
import { fileType } from "../models/rail-model";
import type { Jump } from "./Search";
import { attachmentAction, openAttachment } from "../components/file-actions";

export const fileCategories = () =>
  [
    ["all", t("All files")],
    ["image", t("Images")],
    ["pdf", t("PDFs")],
    ["doc", t("Documents")],
    ["other", t("Other")],
    ["mine", t("Shared by me")],
  ] as const;

export function FileNav(props: { category: string; onCategory: (category: string) => void }) {
  return (
    <nav class="rail-list file-nav" aria-label={t("Files")}>
      <header class="rail-list-header">
        <h2>{t("Files")}</h2>
      </header>
      <For each={fileCategories()}>
        {([id, label]) => (
          <button
            class="file-nav-item"
            classList={{ active: props.category === id }}
            onClick={() => props.onCategory(id)}
          >
            <Document /> {label}
          </button>
        )}
      </For>
    </nav>
  );
}

export default function FileBrowser(props: {
  category: string;
  revision: number;
  conversations: Conversation[];
  onOpen: (jump: Jump) => void;
}) {
  const [query, setQuery] = createSignal("");
  const [sort, setSort] = createSignal("newest");
  const [files, setFiles] = createSignal<StoredFile[]>([]);
  const [more, setMore] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [working, setWorking] = createSignal<string | null>(null);
  const [error, setError] = createSignal("");
  let generation = 0;
  let timer: number | undefined;
  const load = async (append = false) => {
    const gen = ++generation;
    setBusy(true);
    setError("");
    const kind = ["all", "mine"].includes(props.category) ? null : props.category;
    try {
      const rows = await api.browseFiles(
        query(),
        kind,
        sort(),
        props.category === "mine",
        append ? files().length : 0,
      );
      if (gen !== generation) return;
      setFiles(append ? [...files(), ...rows] : rows);
      setMore(rows.length === 100);
    } catch (e) {
      if (gen === generation) setError(errorMessage(e));
    } finally {
      if (gen === generation) setBusy(false);
    }
  };
  createEffect(
    on([() => props.category, sort, query, () => props.revision], () => {
      generation++;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => load(), 120);
    }),
  );
  onCleanup(() => {
    generation++;
    window.clearTimeout(timer);
  });
  const act = async (f: StoredFile, action: "open" | "download" | "jump") => {
    if (working()) return;
    setWorking(f.id);
    setError("");
    try {
      if (action === "jump") {
        if (f.conversation_id && f.ts) {
          const m = await api.message(f.conversation_id, f.ts);
          props.onOpen({
            conversation_id: f.conversation_id,
            ts: f.ts,
            thread_ts: m?.thread_ts ?? null,
          });
        }
        return;
      }
      if (action === "download") await downloadAttachment(f.id);
      else {
        const opened = await openAttachment(f.id, f.ts ? {
          user_id: f.user_id,
          name: people()[f.user_id ?? ""] ?? t("Unknown"),
          ts: f.ts,
          conversation_id: f.conversation_id ?? undefined,
        } : undefined);
        if (!opened) return;
      }
      await load();
    } catch {
      setError(t("Can't download or open this file. It may be unavailable, too large or unsupported."));
    } finally {
      setWorking(null);
    }
  };
  const where = (f: StoredFile) => {
    const c = props.conversations.find((c) => c.id === f.conversation_id);
    if (!c) return f.conversation_id ?? t("Unknown conversation");
    return c.kind === "im" || c.kind === "mpim" ? c.label : `#${c.label}`;
  };
  const available = (f: StoredFile) => !f.hidden_by_limit;
  return (
    <div class="timeline file-browser">
      <header class="conv-header">
        <h2>{fileCategories().find(([id]) => id === props.category)?.[1] ?? t("All files")}</h2>
      </header>
      <div class="file-controls">
        <input
          type="search"
          aria-label={t("Search files")}
          placeholder={t("Search files")}
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
        />
        <select
          aria-label={t("Sort files")}
          value={sort()}
          onChange={(e) => setSort(e.currentTarget.value)}
        >
          <option value="newest">{t("Newest")}</option>
          <option value="name">{t("Name")}</option>
          <option value="size">{t("Size")}</option>
        </select>
      </div>
      <div class="scroller">
        <For each={files()}>
          {(f) => (
            <div class="browser-file" {...fileMenu(() => ({ ...f, permalink: f.file_permalink }), () => f.ts ? {
              user_id: f.user_id, name: people()[f.user_id ?? ""] ?? t("Unknown"), ts: f.ts, conversation_id: f.conversation_id ?? undefined,
            } : undefined)}>
              <button
                class="browser-file-icon"
                data-kind={fileType(f.mimetype)}
                disabled={!available(f) || !!working()}
                onClick={() => act(f, "open")}
                aria-label={fileName(f, t("Untitled file"))}
              >
                <Show when={settings().image_previews && !f.hidden_by_limit}
                  fallback={fileType(f.mimetype) === "pdf" ? "PDF" : <Document size={26} />}>
                  <Show when={f.viewer_kind === "svg" && (f.preview_path || f.local_path)} fallback={
                    <Show when={isRasterImage(f.mimetype) && f.local_path}
                      fallback={fileType(f.mimetype) === "pdf" ? "PDF" : <Document size={26} />}>
                      <img src={fileUrl(f.id)} alt="" loading="lazy" />
                    </Show>
                  }>
                    <SvgPreview file={f} fallback={<Document size={26} />}>
                      {(src, onError) => <Show when={src()} fallback={<Document size={26} />}>
                        <img src={src()} alt="" loading="lazy" onError={onError} />
                      </Show>}
                    </SvgPreview>
                  </Show>
                </Show>
              </button>
              <div class="browser-file-text">
                <button disabled={!available(f) || !!working()} onClick={() => act(f, "open")}>
                  {fileName(f, t("Untitled file"))}
                </button>
                <p class="muted">
                  {people()[f.user_id ?? ""] ?? t("Unknown")} · {where(f)} ·{" "}
                  {f.ts ? tsDate(f.ts).toLocaleDateString(locale()) : ""}
                </p>
                <FileSafety file={f} />
                <Show when={working() === f.id}>
                  <p role="status">{t("Downloading…")}</p>
                </Show>
                <Show when={attachmentAction(f.id).error}>
                  <p class="error" role="alert">{attachmentAction(f.id).error}</p>
                </Show>
                <Show when={f.hidden_by_limit}>
                  <p class="muted">{t("Hidden by Slack's 90-day limit")}</p>
                </Show>
              </div>
              <div class="browser-file-actions">
                <button disabled={!available(f) || !!working()} onClick={() => act(f, "open")}>
                  {f.viewer_kind ? t("Open in viewer") : fileActionLabel(f)}
                </button>
                <Show when={available(f)}><button disabled={!!working() || downloading(f.id)} aria-busy={downloading(f.id)} onClick={() => act(f, "download")}>
                  {downloading(f.id) ? t("Downloading…") : t("Download")}
                </button></Show>
                <button disabled={!!working()} onClick={() => act(f, "jump")}>
                  {t("Jump to message")}
                </button>
              </div>
            </div>
          )}
        </For>
        <Show when={error()}>
          <p class="error view-empty" role="alert">
            {error()}
          </p>
        </Show>
        <Show when={!busy() && files().length === 0 && !error()}>
          <p class="view-empty muted">{t("No files match.")}</p>
        </Show>
        <Show when={more()}>
          <button class="load-more" disabled={busy()} onClick={() => load(true)}>
            {t("Load more")}
          </button>
        </Show>
      </div>
    </div>
  );
}
