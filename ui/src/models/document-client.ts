import type { OfficeResult } from "./document";

export function parseOffice(
  data: ArrayBuffer,
  kind: "xlsx" | "docx",
  signal: AbortSignal,
): Promise<OfficeResult> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) {
      reject(new Error("closed"));
      return;
    }
    const worker = new Worker(
      new URL("../workers/document-worker.ts", import.meta.url),
      { type: "module" },
    );
    const stop = () => {
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      worker.terminate();
    };
    const abort = () => {
      stop();
      reject(new Error("closed"));
    };
    const timer = setTimeout(() => {
      stop();
      reject(new Error("document_limit"));
    }, 15000);
    signal.addEventListener("abort", abort, { once: true });
    worker.onmessage = (
      event: MessageEvent<{ result?: OfficeResult; error?: string }>,
    ) => {
      stop();
      if (event.data.result) resolve(event.data.result);
      else
        reject(
          new Error(
            event.data.error === "limit"
              ? "document_limit"
              : "invalid_document",
          ),
        );
    };
    worker.onerror = (event) => {
      event.preventDefault();
      stop();
      reject(new Error("invalid_document"));
    };
    worker.postMessage({ data, kind }, [data]);
  });
}
