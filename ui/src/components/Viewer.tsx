import { fileMenu } from "./file-menu";
import { DownloadButton } from "./Download";
import FileSafety, { fileActionLabel } from "./FileSafety";
import { fileName } from "../models/file-presentation";
import { createSignal, For, lazy, onCleanup, onMount, Show, Suspense } from "solid-js";
import { t, locale, clock24 } from "../i18n/index";
import { api, fileUrl, tsDate, type FileRef } from "../api";
import Avatar from "./Avatar";
import { viewerFailure } from "../models/document";
export { canView } from "../models/document";

const OfficeDocument = lazy(() => import("./OfficeDocument"));

// In-app viewer for cached images, video, PDFs and Office documents.

/// Who shared the file, for the header.
export type Sender = { user_id: string | null; name: string; ts: string; conversation_id?: string };

export type ViewerFile = Pick<FileRef,
  "id" | "name" | "mimetype" | "local_path" | "can_open" | "viewer_kind" | "hidden_by_limit"
> & { permalink?: string | null };
type Viewing = { file: ViewerFile; from?: Sender };

const [viewing, setViewing] = createSignal<Viewing | null>(null);

export const isViewing = () => viewing() !== null;
export const openViewer = (file: ViewerFile, from?: Sender) => {
  document.querySelectorAll<HTMLVideoElement>(".file-video").forEach((video) => video.pause());
  setViewing({ file, from });
};

export default function Viewer() {
  return <Show when={viewing()} keyed>{(v) => <Overlay v={v} />}</Show>;
}

function Overlay(props: { v: Viewing }) {
  const f = props.v.file;
  const close = () => setViewing(null);
  const [actionError, setActionError] = createSignal("");
  const openDefault = async () => {
    setActionError("");
    try {
      await api.openFile(f.id);
    } catch {
      setActionError(t("Can't download or open this file. It may be unavailable, too large or unsupported."));
    }
  };
  onMount(() => {
    const onKey = (e: KeyboardEvent) => !e.defaultPrevented && e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });
  const when = () => {
    const ts = props.v.from?.ts;
    if (!ts) return null;
    return tsDate(ts).toLocaleString(locale(), {
      hour12: !clock24(),
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };
  return (
    // A preview over the dimmed app, like Slack's, below the window's
    // traffic lights; a click on the dim area closes it.
    <div class="viewer-backdrop" onClick={(e) => e.target === e.currentTarget && close()}>
      <div class="viewer" role="dialog" aria-label={name(f)}>
        <header class="viewer-bar">
          <Show when={props.v.from}>
            {(from) => <Avatar id={from().user_id} name={from().name} size={32} />}
          </Show>
          <div class="viewer-title">
            <div class="viewer-sender">
              <Show when={props.v.from}>{(from) => <strong>{from().name}</strong>}</Show>
              <Show when={when()}>{(w) => <span class="muted">{w()}</span>}</Show>
            </div>
            <div class="viewer-name">{name(f)}</div>
            <FileSafety file={f} />
          </div>
          <button
            class="viewer-btn"
            aria-label={fileActionLabel(f)}
            title={fileActionLabel(f)}
            onClick={openDefault}
          >
            <svg viewBox="0 0 20 20" aria-hidden="true">
              <path d="M11 3h6v6M17 3l-8 8M8 4H4v12h12v-4" />
            </svg>
          </button>
          <DownloadButton id={f.id} hidden={f.hidden_by_limit} class="viewer-btn" />
          <button class="viewer-btn" aria-label={t("Close (Esc)")} title={t("Close (Esc)")} onClick={close}>
            <svg viewBox="0 0 20 20" aria-hidden="true">
              <path d="M5 5l10 10M15 5L5 15" />
            </svg>
          </button>
        </header>
        <Show when={actionError()}>
          <p class="error" role="alert">{actionError()}</p>
        </Show>
        {/* A click on the empty stage closes, like Slack's viewer. */}
        <div class="viewer-stage" tabIndex={0} {...fileMenu(() => f, () => props.v.from)} onClick={(e) => e.target === e.currentTarget && close()}>
          <Show when={f.viewer_kind === "pdf" || f.viewer_kind === "xlsx" || f.viewer_kind === "docx"}
            fallback={<Media file={f} onOpen={openDefault} />}>
            <Show when={f.viewer_kind === "pdf"} fallback={
              <Suspense fallback={<div class="spinner" role="status" aria-label={t("Loading document…")} />}>
                <OfficeDocument file={f} kind={f.viewer_kind === "xlsx" ? "xlsx" : "docx"} onOpen={openDefault} />
              </Suspense>
            }>
              <Pdf file={f} onClose={close} onOpen={openDefault} />
            </Show>
          </Show>
        </div>
      </div>
    </div>
  );
}

function Failure(props: { file: ViewerFile; message: string; onOpen: () => void }) {
  return (
    <div class="viewer-fail" role="alert">
      <p>{props.message}</p>
      <button class="primary" onClick={props.onOpen}>{fileActionLabel(props.file)}</button>
    </div>
  );
}

function Media(props: { file: ViewerFile; onOpen: () => void }) {
  const [src, setSrc] = createSignal(props.file.viewer_kind === "svg" ? "" : fileUrl(props.file.id));
  const [failed, setFailed] = createSignal("");
  let video: HTMLVideoElement | undefined;
  onMount(() => {
    let closed = false;
    let url: string | undefined;
    onCleanup(() => {
      closed = true;
      if (video) {
        video.pause();
        video.removeAttribute("src");
        video.load();
      }
      if (url) URL.revokeObjectURL(url);
    });
    if (props.file.viewer_kind === "svg") {
      // An SVG image document is never inserted into the DOM or navigated to.
      api.documentData(props.file.id)
        .then((bytes) => {
          if (closed) return;
          url = URL.createObjectURL(new Blob([bytes], { type: "image/svg+xml" }));
          setSrc(url);
        })
        .catch((e) => {
          if (!closed) {
            setFailed(viewerFailure(e) === "too_large"
              ? t("This image is too large to preview (over 50 MB). Use the file action above.")
              : t("Can't show this image here."));
          }
        });
    }
  });
  return (
    <Show when={!failed()} fallback={<Failure file={props.file} message={failed()} onOpen={props.onOpen} />}>
      <Show when={src()} fallback={<div class="spinner" role="status" aria-label={t("Loading image…")} />}>
        <Show when={props.file.viewer_kind === "video"} fallback={
          <img
            class="viewer-image"
            src={src()}
            alt={name(props.file)}
            onError={() => setFailed(t("Can't show this image here."))}
          />
        }>
          <video
            ref={video}
            class="viewer-video"
            controls
            playsinline
            preload="metadata"
            src={src()}
            aria-label={name(props.file)}
            onError={() => {
              video?.pause();
              video?.removeAttribute("src");
              video?.load();
              setFailed(t("Can't play this video here. Try opening it in the default app."));
            }}
          />
        </Show>
      </Show>
    </Show>
  );
}

/// Pages of a PDF, drawn by pdf.js as they scroll into view, with a strip
/// of page thumbnails on the left.
function Pdf(props: { file: ViewerFile; onClose: () => void; onOpen: () => void }) {
  const [pages, setPages] = createSignal<{ w: number; h: number }[]>([]);
  const [failed, setFailed] = createSignal<string | null>(null);
  const [current, setCurrent] = createSignal(1);
  let stage!: HTMLDivElement;
  let strip: HTMLDivElement | undefined;

  onMount(async () => {
    // Registered before the first await, so closing mid-load still cleans up.
    let task: import("pdfjs-dist").PDFDocumentLoadingTask | undefined;
    const observers: IntersectionObserver[] = [];
    let closed = false;
    onCleanup(() => {
      closed = true;
      observers.forEach((o) => o.disconnect());
      task?.destroy();
    });
    try {
      // Loaded on first use: pdf.js is large and most sessions never need it.
      const [pdfjs, worker] = await Promise.all([
        import("pdfjs-dist"),
        import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
      ]);
      pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
      const data = await api.documentData(props.file.id);
      task = pdfjs.getDocument({ data });
      const doc = await task.promise;
      if (closed) return;
      const sizes = [];
      for (let i = 1; i <= doc.numPages; i++) {
        const v = (await doc.getPage(i)).getViewport({ scale: 1 });
        sizes.push({ w: v.width, h: v.height });
      }
      if (closed) return;
      setPages(sizes);

      // Pages and thumbnails are drawn once each, when they come near view.
      const lazy = (root: HTMLElement) => {
        const drawn = new Set<Element>();
        const o = new IntersectionObserver(
          (entries) => {
            for (const e of entries) {
              if (!e.isIntersecting || drawn.has(e.target)) continue;
              drawn.add(e.target);
              const canvas = e.target as HTMLCanvasElement;
              draw(doc, Number(canvas.dataset.page), canvas).catch(() => drawn.delete(canvas));
            }
          },
          { root, rootMargin: "800px 0px" },
        );
        root.querySelectorAll("canvas").forEach((c) => o.observe(c));
        observers.push(o);
      };
      lazy(stage);
      if (strip) lazy(strip);
    } catch (e) {
      if (!closed) {
        setFailed(
          viewerFailure(e) === "too_large"
            ? t("This file is too large to preview (over 50 MB). Open it in the default app.")
            : t("Can't show this PDF here."),
        );
      }
    }
  });

  // The current page: the last one whose top is above the upper third.
  const onScroll = () => {
    const mark = stage.scrollTop + stage.clientHeight / 3;
    let n = 1;
    for (const c of stage.querySelectorAll<HTMLCanvasElement>("canvas")) {
      if (c.offsetTop > mark) break;
      n = Number(c.dataset.page);
    }
    if (n !== current()) {
      setCurrent(n);
      strip?.querySelector(`[data-thumb="${n}"]`)?.scrollIntoView({ block: "nearest" });
    }
  };
  const go = (n: number) =>
    stage.querySelector(`canvas[data-page="${n}"]`)?.scrollIntoView({ block: "start" });

  return (
    <div class="viewer-doc">
      <Show when={pages().length > 1}>
        <div class="viewer-thumbs" ref={strip}>
          <For each={pages()}>
            {(p, i) => (
              <button
                class="viewer-thumb"
                classList={{ current: current() === i() + 1 }}
                data-thumb={i() + 1}
                onClick={() => go(i() + 1)}
              >
                <canvas data-page={i() + 1} style={{ "aspect-ratio": `${p.w} / ${p.h}` }} />
                <span>{i() + 1}</span>
              </button>
            )}
          </For>
        </div>
      </Show>
      <div
        class="viewer-pdf"
        ref={stage}
        onScroll={onScroll}
        onClick={(e) => e.target === e.currentTarget && props.onClose()}
      >
        <Show when={failed()}>
          <Failure file={props.file} message={failed()!} onOpen={props.onOpen} />
        </Show>
        <Show when={!failed() && pages().length === 0}>
          <div class="spinner" />
        </Show>
        <For each={pages()}>
          {(p, i) => (
            <canvas
              class="viewer-page"
              data-page={i() + 1}
              style={{ "aspect-ratio": `${p.w} / ${p.h}` }}
            />
          )}
        </For>
      </div>
    </div>
  );
}

async function draw(
  doc: import("pdfjs-dist").PDFDocumentProxy,
  n: number,
  canvas: HTMLCanvasElement,
) {
  const page = await doc.getPage(n);
  const base = page.getViewport({ scale: 1 });
  // Sharp on Retina: render at the shown width times the pixel ratio.
  const scale = (canvas.clientWidth * window.devicePixelRatio) / base.width;
  const viewport = page.getViewport({ scale });
  canvas.width = Math.round(viewport.width);
  canvas.height = Math.round(viewport.height);
  await page.render({ canvas, viewport }).promise;
}

const name = (f: ViewerFile) => fileName(f, t("file"));
