// First-page thumbnails for cached PDFs that Slack gave no preview for
// (mostly files from imported archives). Rendered one at a time so a channel
// full of PDFs cannot stall the timeline, and kept as blob URLs per file.
import { api } from "../api";

const LIMIT = 200;
const thumbs = new Map<string, Promise<string | null>>();
let queue: Promise<unknown> = Promise.resolve();

export function pdfThumb(id: string, width: number): Promise<string | null> {
  const cached = thumbs.get(id);
  if (cached) return cached;
  const next = queue.then(() => render(id, width));
  queue = next.catch(() => {});
  thumbs.set(id, next);
  if (thumbs.size > LIMIT) {
    const [oldest, url] = thumbs.entries().next().value!;
    thumbs.delete(oldest);
    void url.then((u) => u && URL.revokeObjectURL(u));
  }
  return next;
}

async function render(id: string, width: number): Promise<string | null> {
  let task: import("pdfjs-dist").PDFDocumentLoadingTask | undefined;
  try {
    const [pdfjs, worker] = await Promise.all([
      import("pdfjs-dist"),
      import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
    ]);
    pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
    task = pdfjs.getDocument({ data: await api.documentData(id) });
    const page = await (await task.promise).getPage(1);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: (width * window.devicePixelRatio) / base.width });
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(viewport.width);
    canvas.height = Math.round(viewport.height);
    await page.render({ canvas, viewport }).promise;
    const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
    return blob ? URL.createObjectURL(blob) : null;
  } catch {
    // Too large for the viewer limit, damaged or encrypted: just no preview.
    return null;
  } finally {
    void task?.destroy();
  }
}
