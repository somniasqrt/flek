import { createSignal, onCleanup, Show } from "solid-js";
import { api } from "../api";
import { t } from "../i18n/index";
import { revealLabel } from "./FileSafety";

const [busy, setBusy] = createSignal<string[]>([]);
const [notice, setNotice] = createSignal<{ token?: string; error?: string } | null>(null);
export const downloading = (id: string) => busy().includes(id);
let expiry: ReturnType<typeof setTimeout> | undefined;
const notify = (value: { token?: string; error?: string }) => {
  clearTimeout(expiry);
  setNotice(value);
  expiry = setTimeout(() => setNotice(null), 8000);
};

export async function downloadAttachment(id: string) {
  if (downloading(id)) return;
  setBusy((ids) => [...ids, id]);
  try { notify({ token: await api.downloadFile(id) }); }
  catch (e) {
    const message = String(e);
    notify({ error: message.includes("downloads_unavailable")
      ? t("The system Downloads folder could not be found or accessed.")
      : message.includes("downloads_save_failed")
        ? t("Can't save to Downloads. Check folder permissions and free space.")
        : t("Can't download this file. It may be unavailable or too large.") });
  } finally { setBusy((ids) => ids.filter((value) => value !== id)); }
}

export function DownloadButton(props: { id: string; class?: string; hidden?: boolean; disabled?: boolean }) {
  return <Show when={!props.hidden}>
    <button class={props.class ?? "file-download"} disabled={props.disabled || downloading(props.id)} aria-busy={downloading(props.id)}
      aria-label={t("Download to Downloads")} title={t("Download to Downloads")}
      onClick={(e) => { e.stopPropagation(); void downloadAttachment(props.id); }}>
      <Show when={!downloading(props.id)} fallback={<span class="spinner" />}>
        <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6">
          <path d="M10 2v10m-4-4 4 4 4-4M3 13v4h14v-4" />
        </svg>
      </Show>
    </button>
  </Show>;
}

export default function DownloadNotice() {
  onCleanup(() => clearTimeout(expiry));
  return <Show when={notice()}>{(value) => <div class="file-notice" role={value().error ? "alert" : "status"}>
    <span>{value().error ?? t("Saved to Downloads")}</span>
    <Show when={value().token}>{(token) => <button onClick={() => {
      clearTimeout(expiry);
      api.revealDownload(token()).catch(() => notify({ error: t("Could not show the saved copy. Download it again and retry.") }));
    }}>{revealLabel()}</button>}</Show>
    <button aria-label={t("Close")} onClick={() => setNotice(null)}>×</button>
  </div>}</Show>;
}
