import {
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { api, type FileRef } from "../api";
import { locale, t } from "../i18n/index";
import {
  cellFormatter,
  columnLetter,
  ROW_LIMIT,
  COLUMN_LIMIT,
  type OfficeResult,
  type Sheet,
} from "../models/document";
import { parseOffice } from "../models/document-client";
import { sanitizeDocument } from "../models/document-sanitize";
import { fileActionLabel } from "./FileSafety";

export default function OfficeDocument(props: {
  file: FileRef;
  kind: "xlsx" | "docx";
}) {
  const [document, setDocument] = createSignal<OfficeResult>();
  const [error, setError] = createSignal("");
  const [active, setActive] = createSignal(0);
  let tabs!: HTMLDivElement;
  onMount(async () => {
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    try {
      const data = await api.documentData(props.file.id);
      if (controller.signal.aborted) return;
      const result = await parseOffice(data, props.kind, controller.signal);
      if (controller.signal.aborted) return;
      if (result.kind === "docx")
        result.html = sanitizeDocument(result.html, window);
      setDocument(result);
    } catch (e) {
      if (!controller.signal.aborted) {
        const limit = /document_(too_large|limit)/.test(String(e));
        setError(
          limit
            ? t(
                "This document exceeds the viewer's safety limits. Open it in the default app.",
              )
            : t(
                "Can't show this document here. It may be damaged, encrypted or unsupported.",
              ),
        );
      }
    }
  });
  const sheets = () => {
    const d = document();
    return d?.kind === "xlsx" ? d.sheets : [];
  };
  const html = () => {
    const d = document();
    return d?.kind === "docx" ? d.html : "";
  };
  const selectTab = (event: KeyboardEvent) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const n = sheets().length;
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? n - 1
          : (active() + (event.key === "ArrowRight" ? 1 : -1) + n) % n;
    setActive(next);
    tabs.querySelectorAll<HTMLButtonElement>("[role=tab]")[next]?.focus();
  };
  return (
    <div class="viewer-office">
      <Show when={error()}>
        <div class="viewer-fail" role="alert">
          <p>{error()}</p>
          <button
            class="primary"
            onClick={() => api.openFile(props.file.id).catch(() => {})}
          >
            {fileActionLabel(props.file)}
          </button>
        </div>
      </Show>
      <Show when={!document() && !error()}>
        <div
          class="spinner"
          role="status"
          aria-label={t("Loading document…")}
        />
      </Show>
      <Show when={document()?.kind === "docx"}>
        <div class="viewer-word-scroll" tabIndex={0} aria-label={t("Document")}>
          <article class="viewer-word" innerHTML={html()} />
        </div>
      </Show>
      <Show when={document()?.kind === "xlsx"}>
        <div
          class="viewer-sheet-tabs"
          role="tablist"
          aria-label={t("Sheets")}
          ref={tabs}
          onKeyDown={selectTab}
        >
          <For each={sheets()}>
            {(sheet, i) => (
              <button
                role="tab"
                id={`sheet-tab-${i()}`}
                aria-controls={`sheet-panel-${i()}`}
                aria-selected={active() === i()}
                tabIndex={active() === i() ? 0 : -1}
                onClick={() => setActive(i())}
              >
                {sheet.name}
              </button>
            )}
          </For>
        </div>
        <Show when={sheets()[active()]} keyed>
          {(sheet) => (
            <div
              class="viewer-sheet-panel"
              role="tabpanel"
              id={`sheet-panel-${active()}`}
              aria-labelledby={`sheet-tab-${active()}`}
            >
              <Show when={sheet.truncated}>
                <div class="viewer-truncated">
                  <p>
                    {t(
                      "Preview is limited to the first {rows} rows and {columns} columns. Open the full spreadsheet in the default app.",
                      { rows: ROW_LIMIT, columns: COLUMN_LIMIT },
                    )}
                  </p>
                  <button
                    class="primary"
                    onClick={() => api.openFile(props.file.id).catch(() => {})}
                  >
                    {fileActionLabel(props.file)}
                  </button>
                </div>
              </Show>
              <Show
                when={sheet.rows.length > 0}
                fallback={
                  <p class="viewer-truncated">
                    {sheet.truncated
                      ? t("No cells in this preview.")
                      : t("This sheet is empty.")}
                  </p>
                }
              >
                <SheetTable sheet={sheet} />
              </Show>
            </div>
          )}
        </Show>
      </Show>
    </div>
  );
}

// Fixed-height rows bound the DOM even at the 1000 × 100 preview limit.
function SheetTable(props: { sheet: Sheet }) {
  const [scroll, setScroll] = createSignal(0);
  const [height, setHeight] = createSignal(800);
  let scroller!: HTMLDivElement;
  onMount(() => {
    const observer = new ResizeObserver(() => setHeight(scroller.clientHeight));
    observer.observe(scroller);
    onCleanup(() => observer.disconnect());
  });
  const start = () => Math.max(0, Math.floor(scroll() / 32) - 8);
  const end = () =>
    Math.min(props.sheet.rows.length, start() + Math.ceil(height() / 32) + 16);
  const visible = () => props.sheet.rows.slice(start(), end());
  const format = createMemo(() =>
    cellFormatter(locale(), t("True"), t("False")),
  );
  const columns = () =>
    Array.from({ length: props.sheet.rows[0]?.length ?? 0 }, (_, i) =>
      columnLetter(i),
    );
  return (
    <div
      class="viewer-sheet-scroll"
      ref={scroller}
      tabIndex={0}
      aria-label={props.sheet.name}
      onScroll={() => setScroll(scroller.scrollTop)}
    >
      <table
        class="viewer-sheet"
        aria-label={props.sheet.name}
        aria-rowcount={props.sheet.rows.length + 1}
      >
        <thead>
          <tr aria-rowindex={1}>
            <th scope="col" aria-label={t("Row number")} />
            <For each={columns()}>{(col) => <th scope="col">{col}</th>}</For>
          </tr>
        </thead>
        <tbody>
          <tr aria-hidden="true" style={{ height: `${start() * 32}px` }}>
            <td colspan={columns().length + 1} />
          </tr>
          <For each={visible()}>
            {(row, i) => (
              <tr aria-rowindex={start() + i() + 2}>
                <th scope="row">{start() + i() + 1}</th>
                <For each={row}>
                  {(cell) => (
                    <td>
                      <span title={format()(cell)}>{format()(cell)}</span>
                    </td>
                  )}
                </For>
              </tr>
            )}
          </For>
          <tr
            aria-hidden="true"
            style={{ height: `${(props.sheet.rows.length - end()) * 32}px` }}
          >
            <td colspan={columns().length + 1} />
          </tr>
        </tbody>
      </table>
    </div>
  );
}
