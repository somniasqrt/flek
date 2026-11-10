import FileSafety from "./FileSafety";
import { isRasterImage, fileName } from "../models/file-presentation";
import { For, Show } from "solid-js";
import { t } from "../i18n/index";
import { settings } from "../stores/settings";
import { api, fileUrl, type FileRef } from "../api";
import { canView, openViewer, type Sender } from "./Viewer";

// Files of a message: cached images inline, everything else as a card.
// Images and PDFs use the viewer; core decides whether other types open or reveal.

const MAX_W = 480;
const MAX_H = 360;
/// Height of a row of several images.
const ROW_H = 180;

export default function Files(props: { files: FileRef[]; from?: Sender }) {
  const open = (f: FileRef) => {
    if (canView(f)) openViewer(f, props.from);
    else if (f.local_path) api.openFile(f.id).catch(() => {});
    else if (f.permalink) api.openUrl(f.permalink).catch(() => {});
  };
  // Like Slack: images first, then the other files. One image is shown
  // large; several sit in a row at one height. A lone PDF gets a card with
  // the top of its first page; several files are compact cards in a row.
  const images = () =>
    props.files.filter((f) => settings().image_previews && isImage(f) && f.local_path);
  const others = () =>
    props.files.filter((f) => !(settings().image_previews && isImage(f) && f.local_path));
  const lone = () => props.files.length === 1;
  return (
    <div class="files">
      <Show when={images().length > 0}>
        <div class="file-row">
          <For each={images()}>
            {(f) => (
              <Image f={f} src={fileUrl(f.id)} single={images().length === 1} onOpen={open} />
            )}
          </For>
        </div>
      </Show>
      <Show when={others().length > 0}>
        <div class="file-row">
          <For each={others()}>{(f) => <Card f={f} preview={lone()} onOpen={open} />}</For>
        </div>
      </Show>
    </div>
  );
}

const isImage = (f: FileRef) => isRasterImage(f.mimetype) && !f.hidden_by_limit;

function Image(props: { f: FileRef; src: string; single: boolean; onOpen: (f: FileRef) => void }) {
  // Reserve the final size up front so loading images never shift the timeline.
  const size = () => {
    const w = props.f.width ?? MAX_W;
    const h = props.f.height ?? MAX_H;
    const k = props.single ? Math.min(1, MAX_W / w, MAX_H / h) : Math.min(ROW_H / h, MAX_W / w);
    // The height follows from the ratio, so the CSS width cap can shrink it.
    return { width: `${Math.round(w * k)}px`, "aspect-ratio": `${w} / ${h}` };
  };
  return (
    <div class="file-image">
      <button
        class="image"
        style={size()}
        aria-label={name(props.f)}
        onClick={() => props.onOpen(props.f)}
      >
        <img src={props.src} alt={name(props.f)} loading="lazy" />
      </button>
      <span class="file-name">{name(props.f)}</span>
      <FileSafety file={props.f} />
    </div>
  );
}

function Card(props: { f: FileRef; preview: boolean; onOpen: (f: FileRef) => void }) {
  const f = props.f;
  const available = !f.hidden_by_limit && (!!f.local_path || !!f.permalink);
  const preview = () =>
    settings().image_previews && props.preview && !f.hidden_by_limit && !!f.preview_path;
  return (
    <button
      class="file-card"
      classList={{ "with-preview": preview() }}
      disabled={!available}
      onClick={() => props.onOpen(f)}
      aria-label={name(f)}
    >
      <span class="file-head">
        <span class="file-badge" data-kind={kind(f)}>
          {(f.filetype ?? kind(f)).slice(0, 4).toUpperCase()}
        </span>
        <span class="file-text">
          <span class="file-name">{name(f)}</span>
          <FileSafety file={f} />
          <span class="muted">
            {f.hidden_by_limit
              ? t("Hidden by Slack's Free plan")
              : f.local_path
                ? kindLabel(f)
                : t("{kind} · not downloaded", { kind: kindLabel(f) ?? t("File") })}
          </span>
        </span>
      </span>
      {/* Top of the first page of a PDF / a video frame, rendered by Slack. */}
      <Show when={preview()}>
        <span class="file-preview">
          <img src={fileUrl(f.id, true)} alt="" loading="lazy" />
        </span>
      </Show>
    </button>
  );
}

const name = (f: FileRef) => fileName(f, t("file"));

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
  ({
    image: t("Image"),
    video: t("Video"),
    audio: t("Audio"),
    pdf: "PDF",
    archive: t("Archive"),
    sheet: t("Spreadsheet"),
    slides: t("Presentation"),
    doc: t("Document"),
    file: t("File"),
  })[kind(f)];
