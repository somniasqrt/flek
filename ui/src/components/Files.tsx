import FileSafety from "./FileSafety";
import SvgPreview from "./SvgPreview";
import { documentKind } from "../models/document";
import { isRasterImage, isInlineVideo, fileName } from "../models/file-presentation";
import { createSignal, For, Show } from "solid-js";
import { t } from "../i18n/index";
import { settings } from "../stores/settings";
import { fileUrl, type FileRef } from "../api";
import type { Sender } from "./Viewer";
import { attachmentAction, openAttachment } from "./file-actions";

// Files of a message: cached images inline, everything else as a card.
// Core supplies viewer eligibility; other types use the OS open/reveal policy.

const MAX_W = 480;
const MAX_H = 360;
/// Height of a row of several images.
const ROW_H = 180;

export default function Files(props: { files: FileRef[]; from?: Sender }) {
  // Like Slack: images first, then the other files. One image is shown
  // large; several sit in a row at one height. A lone PDF gets a card with
  // the top of its first page; several files are compact cards in a row.
  const images = () =>
    props.files.filter((f) => settings().image_previews && isImage(f));
  const others = () =>
    props.files.filter((f) => !(settings().image_previews && isImage(f)));
  const lone = () => props.files.length === 1;
  return (
    <div class="files">
      <Show when={images().length > 0}>
        <div class="file-row">
          <For each={images()}>
            {(f) => (
              <Attachment f={f} from={props.from} image single={images().length === 1} preview={false} />
            )}
          </For>
        </div>
      </Show>
      <Show when={others().length > 0}>
        <div class="file-row">
          <For each={others()}>{(f) => <Attachment f={f} from={props.from} preview={lone()} />}</For>
        </div>
      </Show>
    </div>
  );
}

const isImage = (f: FileRef) => !f.hidden_by_limit && (
  (isRasterImage(f.mimetype) && !!f.local_path) ||
  (f.viewer_kind === "svg" && !!(f.preview_path || f.local_path))
);

function Image(props: { f: FileRef; src?: string; single: boolean; busy: boolean; onOpen: (f: FileRef) => void; onError?: () => void }) {
  // Reserve the final size up front so loading images never shift the timeline.
  const size = () => {
    const w = Math.max(1, props.f.width ?? props.f.preview_width ?? MAX_W);
    const h = Math.max(1, props.f.height ?? props.f.preview_height ?? MAX_H);
    const k = props.single ? Math.min(1, MAX_W / w, MAX_H / h) : Math.min(ROW_H / h, MAX_W / w);
    // The height follows from the ratio, so the CSS width cap can shrink it.
    return { width: `${Math.round(w * k)}px`, "aspect-ratio": `${w} / ${h}` };
  };
  return (
    <div class="file-image">
      <button
        class="image"
        disabled={props.busy}
        style={size()}
        aria-label={name(props.f)}
        onClick={() => props.onOpen(props.f)}
      >
        <Show when={props.src}>
          <img src={props.src} alt={name(props.f)} loading="lazy" onError={props.onError} />
        </Show>
      </button>
      <span class="file-name">{name(props.f)}</span>
      <FileSafety file={props.f} />
    </div>
  );
}

function Attachment(props: { f: FileRef; from?: Sender; preview: boolean; image?: boolean; single?: boolean }) {
  const [failed, setFailed] = createSignal(false);
  const busy = () => attachmentAction(props.f.id).busy;
  const error = () => attachmentAction(props.f.id).error;
  const open = () => {
    if (!props.f.hidden_by_limit) void openAttachment(props.f.id, props.from);
  };
  return (
    <div class="video-or-card" aria-busy={busy()}>
      <Show when={props.image} fallback={
        <Show when={isInlineVideo(props.f) && !failed()} fallback={<Card {...props} busy={busy()} onOpen={open} />}>
          <video
            class="file-video"
            controls
            preload="metadata"
            playsinline
            aria-label={name(props.f)}
            src={fileUrl(props.f.id)}
            poster={props.f.preview_path ? fileUrl(props.f.id, true) : undefined}
            onError={() => setFailed(true)}
          />
          <div class="file-video-actions">
            <button class="file-name" disabled={busy()} onClick={open}>{name(props.f)}</button>
            <button disabled={busy()} onClick={open} aria-label={t("Open in viewer")} title={t("Open in viewer")}>
              <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
                <path d="M7 3H3v4M13 3h4v4M3 13v4h4M17 13v4h-4" fill="none" stroke="currentColor" stroke-width="1.6" />
              </svg>
            </button>
          </div>
          <FileSafety file={props.f} />
        </Show>
      }>
        <Show when={props.f.viewer_kind === "svg"} fallback={
          <Image f={props.f} src={fileUrl(props.f.id)} single={!!props.single} busy={busy()} onOpen={open} />
        }>
          <SvgPreview file={props.f} fallback={<Card {...props} preview={false} busy={busy()} onOpen={open} />}>
            {(src, onError) => <Image f={props.f} src={src()} single={!!props.single} busy={busy()} onOpen={open} onError={onError} />}
          </SvgPreview>
        </Show>
      </Show>
      <Show when={busy()}><p role="status">{t("Downloading…")}</p></Show>
      <Show when={error()}><p class="error" role="alert">{error()}</p></Show>
    </div>
  );
}

function Card(props: { f: FileRef; preview: boolean; busy: boolean; onOpen: (f: FileRef) => void }) {
  const f = props.f;
  const available = !f.hidden_by_limit;
  const preview = () =>
    settings().image_previews && props.preview && !f.hidden_by_limit && !!f.preview_path;
  return (
    <button
      class="file-card"
      classList={{ "with-preview": preview() }}
      disabled={!available || props.busy}
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
      {/* Cached document preview / video frame, when Slack already supplied one. */}
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
  if (documentKind(f) === "xlsx") return "sheet";
  if (documentKind(f) === "docx") return "doc";
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
