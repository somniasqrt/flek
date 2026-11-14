import {
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type JSX,
} from "solid-js";
import { api } from "../api";
import { locale, t } from "../i18n/index";
import {
  cellFormatter,
  columnLetter,
  ROW_LIMIT,
  COLUMN_LIMIT,
  type OfficeResult,
  type Sheet,
  type SheetStyle,
  type SheetBorder,
} from "../models/document";
import { parseOffice } from "../models/document-client";
import { viewerFailure } from "../models/document";
import { sanitizeDocument } from "../models/document-sanitize";
import { fileActionLabel } from "./FileSafety";
import type { ViewerFile } from "./Viewer";

export default function OfficeDocument(props: {
  file: ViewerFile;
  onOpen: () => void;
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
        const failure = viewerFailure(e);
        setError(
          failure === "too_large"
            ? t("This file is too large to preview (over 50 MB). Open it in the default app.")
            : failure === "limit"
              ? t("This document is too large or complex to preview here. Open it in the default app.")
              : t("Can't show this document here. It may be damaged, encrypted or unsupported."),
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
            onClick={props.onOpen}
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
                    onClick={props.onOpen}
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

const ROW_HEADER = 48;
const HEADER_HEIGHT = 24;
const offsets = (sizes: number[]) => {
  const result = [0];
  for (const size of sizes) result.push(result.at(-1)! + size);
  return result;
};
const rowAt = (positions: number[], y: number) => {
  let lo = 0, hi = positions.length - 1;
  while (lo < hi) {
    const mid = Math.floor((lo + hi + 1) / 2);
    if (positions[mid] <= y) lo = mid;
    else hi = mid - 1;
  }
  return Math.min(lo, positions.length - 2);
};
const borderCss = (border: SheetBorder | undefined) => border
  ? `${border.width}px ${border.style} ${border.color}` : undefined;
function cellCss(style: SheetStyle): JSX.CSSProperties {
  return {
    "background-color": style.fill,
    "border-left": borderCss(style.left), "border-right": borderCss(style.right),
    "border-top": borderCss(style.top), "border-bottom": borderCss(style.bottom),
  };
}
function textCss(style: SheetStyle, numeric: boolean): JSX.CSSProperties {
  return {
    color: style.color,
    "font-weight": style.bold ? "700" : "400",
    "font-style": style.italic ? "italic" : "normal",
    "text-decoration": style.underline ? "underline" : "none",
    "font-size": `${style.fontSize ?? 11 * 96 / 72}px`,
    "text-align": style.horizontal ?? (numeric ? "right" : "left"),
    "align-items": style.vertical === "top" ? "flex-start" : style.vertical === "middle" ? "center" : "flex-end",
    "white-space": style.wrap ? "pre-wrap" : "pre",
    "overflow-wrap": style.wrap ? "anywhere" : "normal",
  };
}

// Prefix sums keep explicit file heights and hidden rows out of the scroll cost.
function SheetTable(props: { sheet: Sheet }) {
  const [scroll, setScroll] = createSignal(0);
  const [height, setHeight] = createSignal(800);
  let scroller!: HTMLDivElement;
  onMount(() => {
    const observer = new ResizeObserver(() => setHeight(scroller.clientHeight));
    observer.observe(scroller);
    setHeight(scroller.clientHeight);
    onCleanup(() => observer.disconnect());
  });
  const geometry = createMemo(() => {
    const sheet = props.sheet, layout = sheet.layout;
    const widths = layout?.columnWidths ?? Array(sheet.rows[0]?.length ?? 0).fill(64);
    const heights = layout?.rowHeights ?? Array(sheet.rows.length).fill(20);
    const x = offsets(widths), y = offsets(heights);
    const columns = widths.flatMap((width, i) => width > 0 ? [i] : []);
    const covered = new Int32Array(sheet.rows.length * COLUMN_LIMIT).fill(-1);
    const merges = layout?.merges ?? [];
    merges.forEach((merge, index) => {
      for (let r = merge.row; r <= merge.endRow; r++)
        for (let c = merge.column; c <= merge.endColumn; c++) covered[r * COLUMN_LIMIT + c] = index;
    });
    // Precompute spill boundaries once per sheet, rather than on every scroll.
    const spill = sheet.rows.map((row, r) => {
      let next = widths.length;
      const ends = Array(widths.length).fill(0) as number[];
      for (let c = widths.length - 1; c >= 0; c--) {
        ends[c] = x[next] - x[c];
        if (widths[c] && (row[c] !== null && row[c] !== "" || covered[r * COLUMN_LIMIT + c] >= 0)) next = c;
      }
      return ends;
    });
    return { widths, heights, x, y, columns, covered, merges, spill };
  });
  const visible = createMemo(() => {
    const g = geometry();
    const start = Math.max(0, rowAt(g.y, Math.max(0, scroll() - HEADER_HEIGHT)) - 8);
    const end = Math.min(props.sheet.rows.length, rowAt(g.y, scroll() + height()) + 9);
    const rows = new Set<number>();
    for (let r = start; r < end; r++) if (g.heights[r] > 0) rows.add(r);
    // A merge remains visible even after its anchor scrolls above the window.
    for (const m of g.merges)
      if (g.y[m.endRow + 1] > g.y[start] && g.y[m.row] < g.y[end]) rows.add(m.row);
    return [...rows].sort((a, b) => a - b);
  });
  const format = createMemo(() => cellFormatter(locale(), t("True"), t("False")));
  const cells = (r: number) => {
    const g = geometry();
    return g.widths.flatMap((width, c) => {
      const merge = g.merges[g.covered[r * COLUMN_LIMIT + c]];
      if (merge && (merge.row !== r || merge.column !== c)) return [];
      if (merge ? g.x[merge.endColumn + 1] === g.x[c] : width === 0) return [];
      return [{ column: c, merge }];
    });
  };
  const styleFor = (r: number, c: number) => {
    const layout = props.sheet.layout;
    return layout?.styles[layout.cellStyles[r * COLUMN_LIMIT + c] ?? 0] ?? {};
  };
  return (
    <div
      class="viewer-sheet-scroll"
      ref={scroller}
      tabIndex={0}
      aria-label={props.sheet.name}
      onScroll={() => setScroll(scroller.scrollTop)}
    >
      <div
        class="viewer-sheet"
        role="table"
        aria-label={props.sheet.name}
        aria-rowcount={props.sheet.rows.length + 1}
        aria-colcount={geometry().widths.length + 1}
        style={{ width: `${ROW_HEADER + geometry().x.at(-1)!}px` }}
      >
        <div class="viewer-sheet-head" role="row" aria-rowindex={1}>
          <div class="viewer-sheet-corner" role="columnheader" aria-label={t("Row number")} />
          <For each={geometry().columns}>{(c) => (
            <div role="columnheader" aria-colindex={c + 2}
              style={{ left: `${ROW_HEADER + geometry().x[c]}px`, width: `${geometry().widths[c]}px` }}>
              {columnLetter(c)}
            </div>
          )}</For>
        </div>
        <div class="viewer-sheet-body" role="rowgroup" style={{ height: `${geometry().y.at(-1)!}px` }}>
          <For each={visible()}>{(r) => (
            <div class="viewer-sheet-row" role="row" aria-rowindex={r + 2}
              style={{ top: `${geometry().y[r]}px`, height: `${geometry().heights[r]}px` }}>
              <div class="viewer-sheet-row-number" role="rowheader" aria-colindex={1}>{r + 1}</div>
              <For each={cells(r)}>{({ column: c, merge }) => {
                const value = props.sheet.rows[r]?.[c] ?? null;
                const anchorStyle = styleFor(r, c);
                const style = merge ? {
                  ...anchorStyle,
                  right: styleFor(r, merge.endColumn).right ?? anchorStyle.right,
                  bottom: styleFor(merge.endRow, c).bottom ?? anchorStyle.bottom,
                } : anchorStyle;
                const width = geometry().x[(merge?.endColumn ?? c) + 1] - geometry().x[c];
                const cellHeight = geometry().y[(merge?.endRow ?? r) + 1] - geometry().y[r];
                const numeric = typeof value === "number" || value instanceof Date;
                const spills = typeof value === "string" && !style.wrap && !merge && (!style.horizontal || style.horizontal === "left");
                return (
                  <div class="viewer-sheet-cell" role="cell" aria-colindex={c + 2}
                    aria-colspan={merge ? merge.endColumn - c + 1 : undefined}
                    aria-rowspan={merge ? merge.endRow - r + 1 : undefined}
                    style={{ ...cellCss(style), left: `${ROW_HEADER + geometry().x[c]}px`, width: `${width}px`, height: `${cellHeight}px` }}>
                    <Show when={value !== null && value !== ""}>
                      <span class="viewer-sheet-cell-text" title={format()(value)}
                        style={{ ...textCss(style, numeric), width: `${spills ? Math.max(width, geometry().spill[r][c]) : width}px`, height: `${cellHeight}px` }}>
                        <span>{format()(value)}</span>
                      </span>
                    </Show>
                  </div>
                );
              }}</For>
            </div>
          )}</For>
        </div>
      </div>
    </div>
  );
}
