// In-app file viewer, over the whole window: images and PDFs from the cache.
// Other files still open in their default app.
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { api, fileUrl, tsDate, type FileRef } from "./api";
import Avatar from "./Avatar";

/// Who shared the file, for the header.
export type Sender = { user_id: string | null; name: string; ts: string };

type Viewing = { file: FileRef; from?: Sender };

const [viewing, setViewing] = createSignal<Viewing | null>(null);

const isPdf = (f: FileRef) =>
  f.mimetype === "application/pdf" || f.filetype?.toLowerCase() === "pdf";
const isImage = (f: FileRef) => !!f.mimetype?.startsWith("image/");

/// Whether the viewer can show `f` (it has to be downloaded).
export const canView = (f: FileRef) =>
  !f.hidden_by_limit && !!f.local_path && (isImage(f) || isPdf(f));

export const openViewer = (file: FileRef, from?: Sender) => setViewing({ file, from });

export default function Viewer() {
  return <Show when={viewing()}>{(v) => <Overlay v={v()} />}</Show>;
}

function Overlay(props: { v: Viewing }) {
  const f = props.v.file;
  const close = () => setViewing(null);
  onMount(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });
  const when = () => {
    const ts = props.v.from?.ts;
    if (!ts) return null;
    return tsDate(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };
  return (
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
        </div>
        <button class="viewer-btn" title="Open in default app" onClick={() => api.openFile(f.id).catch(() => {})}>
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M11 3h6v6M17 3l-8 8M8 4H4v12h12v-4" />
          </svg>
        </button>
        <button class="viewer-btn" title="Close (Esc)" onClick={close}>
          <svg viewBox="0 0 20 20" aria-hidden="true">
            <path d="M5 5l10 10M15 5L5 15" />
          </svg>
        </button>
      </header>
      {/* A click on the empty stage closes, like Slack's viewer. */}
      <div class="viewer-stage" onClick={(e) => e.target === e.currentTarget && close()}>
        <Show when={isPdf(f)} fallback={<img class="viewer-image" src={fileUrl(f.id)} alt={name(f)} />}>
          <Pdf file={f} onClose={close} />
        </Show>
      </div>
    </div>
  );
}

/// Pages of a PDF, drawn by pdf.js as they scroll into view, with a strip
/// of page thumbnails on the left.
function Pdf(props: { file: FileRef; onClose: () => void }) {
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
      const data = await api.pdfData(props.file.id);
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
      if (!closed) setFailed(String(e));
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
          <div class="viewer-fail">
            <p>Can't show this PDF here.</p>
            <p class="muted">{failed()}</p>
            <button class="primary" onClick={() => api.openFile(props.file.id).catch(() => {})}>
              Open in default app
            </button>
          </div>
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

async function draw(doc: import("pdfjs-dist").PDFDocumentProxy, n: number, canvas: HTMLCanvasElement) {
  const page = await doc.getPage(n);
  const base = page.getViewport({ scale: 1 });
  // Sharp on Retina: render at the shown width times the pixel ratio.
  const scale = (canvas.clientWidth * window.devicePixelRatio) / base.width;
  const viewport = page.getViewport({ scale });
  canvas.width = Math.round(viewport.width);
  canvas.height = Math.round(viewport.height);
  await page.render({ canvas, viewport }).promise;
}

const name = (f: FileRef) => f.title || f.name || "file";
