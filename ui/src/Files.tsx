// Files of a message: cached images inline, everything else as a card.
// Images and PDFs open in the in-app viewer, other files in their default app.
import { For, Show } from "solid-js";
import { api, fileUrl, type FileRef } from "./api";
import { canView, openViewer, type Sender } from "./Viewer";

const MAX_W = 360;
const MAX_H = 280;

export default function Files(props: { files: FileRef[]; from?: Sender }) {
  const open = (f: FileRef) => {
    if (canView(f)) openViewer(f, props.from);
    else if (f.local_path) api.openFile(f.id).catch(() => {});
    else if (f.permalink) api.openUrl(f.permalink).catch(() => {});
  };
  return (
    <div class="files">
      <For each={props.files}>
        {(f) => (
          <Show when={isImage(f) && f.local_path} fallback={<Card f={f} onOpen={open} />}>
            <Image f={f} src={fileUrl(f.id)} onOpen={open} />
          </Show>
        )}
      </For>
    </div>
  );
}

const isImage = (f: FileRef) => !!f.mimetype?.startsWith("image/") && !f.hidden_by_limit;

function Image(props: { f: FileRef; src: string; onOpen: (f: FileRef) => void }) {
  // Reserve the final size up front so loading images never shift the timeline.
  const size = () => {
    const w = props.f.width ?? MAX_W;
    const h = props.f.height ?? MAX_H;
    const k = Math.min(1, MAX_W / w, MAX_H / h);
    return { width: `${Math.round(w * k)}px`, height: `${Math.round(h * k)}px` };
  };
  return (
    <button class="image" style={size()} title={name(props.f)} onClick={() => props.onOpen(props.f)}>
      <img src={props.src} alt={name(props.f)} loading="lazy" />
    </button>
  );
}

function Card(props: { f: FileRef; onOpen: (f: FileRef) => void }) {
  const f = props.f;
  const available = !f.hidden_by_limit && (!!f.local_path || !!f.permalink);
  return (
    <button
      class="file-card"
      classList={{ "with-preview": !!f.preview_path && !f.hidden_by_limit }}
      disabled={!available}
      onClick={() => props.onOpen(f)}
      title={name(f)}
    >
      <span class="file-head">
        <span class="file-badge" data-kind={kind(f)}>
          {(f.filetype ?? kind(f)).slice(0, 4).toUpperCase()}
        </span>
        <span class="file-text">
          <span class="file-name">{name(f)}</span>
          <span class="muted">
            {f.hidden_by_limit
              ? "Hidden by Slack's Free plan"
              : [kindLabel(f), size(f.size), f.local_path ? null : "not downloaded"]
                  .filter(Boolean)
                  .join(" · ")}
          </span>
        </span>
      </span>
      {/* First page of a PDF / a video frame, rendered by Slack and cached. */}
      <Show when={!f.hidden_by_limit && f.preview_path}>
        <span class="file-preview">
          <img src={fileUrl(f.id, true)} alt="" loading="lazy" />
        </span>
      </Show>
    </button>
  );
}

const name = (f: FileRef) => f.title || f.name || "file";

function kind(f: FileRef): string {
  const m = f.mimetype ?? "";
  const t = (f.filetype ?? "").toLowerCase();
  if (m.startsWith("image/")) return "image";
  if (m.startsWith("video/")) return "video";
  if (m.startsWith("audio/")) return "audio";
  if (t === "pdf" || m === "application/pdf") return "pdf";
  if (/^(zip|rar|7z|gz|tar|gzip)$/.test(t)) return "archive";
  if (/^(xls|xlsx|csv|gsheet|numbers)$/.test(t)) return "sheet";
  if (/^(ppt|pptx|key|gpres)$/.test(t)) return "slides";
  if (/^(doc|docx|odt|rtf|txt|md|gdoc|pages)$/.test(t)) return "doc";
  return "file";
}

const kindLabel = (f: FileRef) =>
  ({ image: "Image", video: "Video", audio: "Audio", pdf: "PDF", archive: "Archive", sheet: "Spreadsheet", slides: "Presentation", doc: "Document", file: "File" })[kind(f)];

function size(bytes: number | null): string | null {
  if (bytes == null) return null;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
