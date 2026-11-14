import { Parser } from "saxen";
import { strFromU8 } from "fflate";
import {
  COLUMN_LIMIT,
  ROW_LIMIT,
  type Sheet,
  type SheetBorder,
  type SheetLayout,
  type SheetMerge,
  type SheetStyle,
} from "./document.ts";

// Optional presentation metadata fails closed independently of saved values.
export const STYLE_LIMIT = 2048;
export const MERGE_LIMIT = 1024;
export const COLUMN_RANGE_LIMIT = 1024;
const METADATA_NODE_LIMIT = 20000;
const DEFAULT_WIDTH = 64;
const DEFAULT_HEIGHT = 20;
const exceeded = () => { throw new Error("sheet_metadata_limit"); };
const integer = (value: string | undefined, max: number) => {
  if (value === undefined || !/^\d+$/.test(value)) return undefined;
  const n = Number(value);
  return Number.isSafeInteger(n) && n <= max ? n : undefined;
};
const numeric = (value: string | undefined, min: number, max: number) => {
  if (value === undefined || value.trim() === "") return undefined;
  const n = Number(value);
  return Number.isFinite(n) && n >= min && n <= max ? n : undefined;
};
const enabled = (value: string | undefined) => value === "1" || value === "true";
const widthPx = (width: number) =>
  Math.floor(width < 1 ? width * 12 + 0.5 : width * 7 + 0.5) + (width < 1 ? 0 : 5);
const pointsPx = (points: number) => Math.round(points * 96 / 72 * 100) / 100;

type Attributes = Record<string, string>;
type XmlNode = { tag: string; attrs: Attributes; children: XmlNode[] };
function scan(bytes: Uint8Array, open: (tag: string, attrs: Attributes, parents: string[]) => void,
  close: () => void = () => {}) {
  const parents: string[] = [];
  const parser = new Parser();
  parser.on("error", exceeded);
  parser.on("warn", exceeded);
  parser.on("openTag", (name, raw, decode) => {
    const tag = name.split(":").pop()!;
    const attrs: Attributes = Object.create(null);
    for (const [key, value] of Object.entries(raw()))
      attrs[key.split(":").pop()!] = decode(value);
    open(tag, attrs, parents);
    parents.push(tag);
  });
  parser.on("closeTag", () => { parents.pop(); close(); });
  parser.parse(strFromU8(bytes));
}
function tree(bytes: Uint8Array) {
  const root: XmlNode = { tag: "", attrs: {}, children: [] };
  const stack = [root];
  let nodes = 0;
  scan(bytes, (tag, attrs) => {
    if (++nodes > METADATA_NODE_LIMIT) exceeded();
    const node = { tag, attrs, children: [] };
    stack.at(-1)!.children.push(node);
    stack.push(node);
  }, () => { stack.pop(); });
  return root.children[0] ?? root;
}
const child = (node: XmlNode | undefined, tag: string) => node?.children.find((n) => n.tag === tag);
const list = (node: XmlNode, tag: string) => child(node, tag)?.children ?? [];
const rgb = (value: string | undefined) =>
  value && /^(?:[0-9a-f]{2})?[0-9a-f]{6}$/i.test(value)
    ? `#${value.slice(-6).toLowerCase()}` : undefined;
const indexed = (
  "000000 ffffff ff0000 00ff00 0000ff ffff00 ff00ff 00ffff " +
  "000000 ffffff ff0000 00ff00 0000ff ffff00 ff00ff 00ffff " +
  "800000 008000 000080 808000 800080 008080 c0c0c0 808080 " +
  "9999ff 993366 ffffcc ccffff 660066 ff8080 0066cc cccccc " +
  "000080 ff00ff ffff00 00ffff 800080 800000 008080 0000ff " +
  "00ccff ccffff ccffcc ffff99 99ccff ff99cc cc99ff ffcc99 " +
  "3366ff 33cccc 99cc00 ffcc00 ff9900 ff6600 666699 969696 " +
  "003366 339966 003300 333300 993300 993366 333399 333333"
).split(" ").map((c) => `#${c}`);

// Excel tint changes HSL luminance, preserving hue and saturation.
function tintColor(color: string, tint: number) {
  if (!tint) return color;
  const channels = [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16) / 255);
  const max = Math.max(...channels), min = Math.min(...channels);
  const light = (max + min) / 2, delta = max - min;
  const saturation = delta === 0 ? 0 : delta / (1 - Math.abs(2 * light - 1));
  let hue = 0;
  if (delta) {
    const [r, g, b] = channels;
    hue = max === r ? ((g - b) / delta + 6) % 6 : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4;
  }
  const l = tint < 0 ? light * (1 + tint) : light * (1 - tint) + tint;
  const c = (1 - Math.abs(2 * l - 1)) * saturation;
  const x = c * (1 - Math.abs(hue % 2 - 1));
  const parts = hue < 1 ? [c, x, 0] : hue < 2 ? [x, c, 0] : hue < 3 ? [0, c, x] : hue < 4 ? [0, x, c] : hue < 5 ? [x, 0, c] : [c, 0, x];
  return "#" + parts.map((v) => Math.round((v + l - c / 2) * 255).toString(16).padStart(2, "0")).join("");
}
function themeColors(bytes: Uint8Array | undefined): (string | undefined)[] {
  if (!bytes) return [];
  const root = tree(bytes);
  const scheme = child(child(root, "themeElements"), "clrScheme");
  return ["lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"].map((tag) => {
    const node = child(scheme, tag)?.children[0];
    return node?.tag === "srgbClr" ? rgb(node.attrs.val) : node?.tag === "sysClr" ? rgb(node.attrs.lastClr) : undefined;
  });
}
function parseStyles(bytes: Uint8Array | undefined, theme: (string | undefined)[]): SheetStyle[] {
  if (!bytes) return [{}];
  const root = tree(bytes);
  const palette = list(child(root, "colors") ?? root, "indexedColors");
  const color = (node: XmlNode | undefined) => {
    if (!node) return undefined;
    const a = node.attrs;
    const i = integer(a.indexed, 63), t = integer(a.theme, 11);
    const base = rgb(a.rgb) ?? (i === undefined ? undefined : rgb(palette[i]?.attrs.rgb) ?? indexed[i]) ?? (t === undefined ? undefined : theme[t]);
    return base ? tintColor(base, numeric(a.tint, -1, 1) ?? 0) : undefined;
  };
  const fonts = list(root, "fonts"), fills = list(root, "fills"), borders = list(root, "borders"), xfs = list(root, "cellXfs");
  if ([fonts, fills, borders, xfs, list(root, "cellStyleXfs")].some((items) => items.length > STYLE_LIMIT)) exceeded();
  const border = (node: XmlNode | undefined): SheetBorder | undefined => {
    const style = node?.attrs.style;
    if (!style || !["thin", "medium", "thick", "dashed", "dotted", "double", "hair", "mediumDashed", "dashDot", "mediumDashDot", "dashDotDot", "mediumDashDotDot", "slantDashDot"].includes(style)) return undefined;
    return {
      style: style === "double" ? "double" : style === "dotted" || style === "hair" ? "dotted" : /dash/i.test(style) ? "dashed" : "solid",
      width: style === "thick" || style === "double" ? 3 : /medium/i.test(style) ? 2 : 1,
      color: color(child(node, "color")) ?? "#000000",
    };
  };
  return xfs.length ? xfs.map((xf) => {
    const font = fonts[integer(xf.attrs.fontId, STYLE_LIMIT - 1) ?? 0];
    const fill = child(fills[integer(xf.attrs.fillId, STYLE_LIMIT - 1) ?? 0], "patternFill");
    const edges = borders[integer(xf.attrs.borderId, STYLE_LIMIT - 1) ?? 0];
    const alignment = child(xf, "alignment")?.attrs ?? {};
    const flag = (tag: string) => { const n = child(font, tag); return !!n && n.attrs.val !== "0" && n.attrs.val !== "false" && n.attrs.val !== "none"; };
    const fontSize = numeric(child(font, "sz")?.attrs.val, 1, 409);
    const horizontal = alignment.horizontal;
    const vertical = alignment.vertical;
    return {
      ...(xf.attrs.applyFont === "0" ? {} : {
        bold: flag("b"), italic: flag("i"), underline: flag("u"),
        fontSize: fontSize === undefined ? undefined : pointsPx(fontSize),
        color: color(child(font, "color")),
      }),
      fill: xf.attrs.applyFill === "0" || fill?.attrs.patternType !== "solid" ? undefined : color(child(fill, "fgColor")),
      ...(xf.attrs.applyBorder === "0" ? {} : {
        left: border(child(edges, "left")), right: border(child(edges, "right")),
        top: border(child(edges, "top")), bottom: border(child(edges, "bottom")),
      }),
      ...(xf.attrs.applyAlignment === "0" ? {} : {
        horizontal: horizontal === "left" || horizontal === "center" || horizontal === "right" || horizontal === "justify" ? horizontal : undefined,
        vertical: vertical === "top" || vertical === "bottom" ? vertical : vertical === "center" ? "middle" : undefined,
        wrap: enabled(alignment.wrapText),
      }),
    };
  }) : [{}];
}
const reference = (value: string | undefined, full = false) => {
  const match = value?.match(/^([A-Z]{1,3})([1-9][0-9]{0,6})$/);
  if (!match) return undefined;
  const column = [...match[1]].reduce((n, c) => n * 26 + c.charCodeAt(0) - 64, 0) - 1;
  const row = Number(match[2]) - 1;
  return row < (full ? 1048576 : ROW_LIMIT) && column < (full ? 16384 : COLUMN_LIMIT) ? { row, column } : undefined;
};
function parseSheet(bytes: Uint8Array, styles: SheetStyle[]) {
  const columnWidths = Array(COLUMN_LIMIT).fill(DEFAULT_WIDTH) as number[];
  const rowHeights = Array(ROW_LIMIT).fill(DEFAULT_HEIGHT) as number[];
  const cellStyles: Record<number, number> = Object.create(null);
  const columnStyles: number[] = [];
  const merges: SheetMerge[] = [];
  const occupied = new Uint8Array(ROW_LIMIT * COLUMN_LIMIT);
  let ranges = 0, mergeCount = 0, row = -1, column = -1;
  let rowCount = 0, columnCount = 0, truncated = false;
  let defaultHeight = DEFAULT_HEIGHT, defaultHidden = false, rowStyle = 0;
  const styleId = (value: string | undefined) => integer(value, styles.length - 1);
  scan(bytes, (tag, a, parents) => {
    const parent = parents.at(-1);
    if (tag === "sheetFormatPr") {
      const width = numeric(a.defaultColWidth, 0, 255);
      const base = numeric(a.baseColWidth, 0, 255);
      columnWidths.fill(width === undefined ? base === undefined ? DEFAULT_WIDTH : widthPx(base) : widthPx(width));
      defaultHeight = pointsPx(numeric(a.defaultRowHeight, 0, 409) ?? 15);
      defaultHidden = enabled(a.zeroHeight);
      rowHeights.fill(defaultHidden ? 0 : defaultHeight);
    } else if (tag === "col" && parent === "cols") {
      if (++ranges > COLUMN_RANGE_LIMIT) exceeded();
      const min = integer(a.min, 16384), max = integer(a.max, 16384);
      if (!min || !max || min > max) return;
      const width = numeric(a.width, 0, 255);
      for (let c = min - 1; c < Math.min(max, COLUMN_LIMIT); c++) {
        if (enabled(a.hidden)) columnWidths[c] = 0;
        else if (width !== undefined) columnWidths[c] = widthPx(width);
        columnStyles[c] = styleId(a.style) ?? 0;
      }
    } else if (tag === "row" && parents.includes("sheetData")) {
      row = a.r === undefined ? row + 1 : (integer(a.r, ROW_LIMIT) ?? 0) - 1;
      column = -1;
      rowStyle = styleId(a.s) ?? 0;
      if (row < 0 || row >= ROW_LIMIT) return;
      rowCount = Math.max(rowCount, row + 1);
      const height = numeric(a.ht, 0, 409);
      rowHeights[row] = enabled(a.hidden) || (defaultHidden && a.hidden === undefined && height === undefined) ? 0 : height === undefined ? defaultHeight : pointsPx(height);
      if (rowStyle) for (let c = 0; c < COLUMN_LIMIT; c++) cellStyles[row * COLUMN_LIMIT + c] = rowStyle;
    } else if (tag === "c" && parent === "row" && parents.includes("sheetData")) {
      column = a.r === undefined ? column + 1 : reference(a.r)?.column ?? -1;
      if (row < 0 || row >= ROW_LIMIT || column < 0 || column >= COLUMN_LIMIT) return;
      columnCount = Math.max(columnCount, column + 1);
      cellStyles[row * COLUMN_LIMIT + column] = styleId(a.s) ?? (rowStyle || columnStyles[column] || 0);
    } else if (tag === "mergeCell" && parent === "mergeCells") {
      if (++mergeCount > MERGE_LIMIT) exceeded();
      const parts = a.ref?.split(":");
      const first = reference(parts?.[0]), last = reference(parts?.[1], true);
      if (!first || !last || first.row > last.row || first.column > last.column) return;
      if (last.row >= ROW_LIMIT || last.column >= COLUMN_LIMIT) truncated = true;
      last.row = Math.min(last.row, ROW_LIMIT - 1);
      last.column = Math.min(last.column, COLUMN_LIMIT - 1);
      for (let r = first.row; r <= last.row; r++) for (let c = first.column; c <= last.column; c++) {
        if (occupied[r * COLUMN_LIMIT + c]) exceeded();
        occupied[r * COLUMN_LIMIT + c] = 1;
      }
      merges.push({ ...first, endRow: last.row, endColumn: last.column });
      rowCount = Math.max(rowCount, last.row + 1);
      columnCount = Math.max(columnCount, last.column + 1);
    }
  });
  // Column styles also apply to empty cells, without allocating style objects per cell.
  for (let r = 0; r < ROW_LIMIT; r++) for (let c = 0; c < COLUMN_LIMIT; c++)
    if (columnStyles[c] && cellStyles[r * COLUMN_LIMIT + c] === undefined)
      cellStyles[r * COLUMN_LIMIT + c] = columnStyles[c];
  const layout: SheetLayout = { columnWidths, rowHeights, cellStyles, merges, styles };
  return { layout, rowCount, columnCount, truncated };
}

/** Only call with entries returned by boundedArchive; no ZIP reads or resources. */
export function spreadsheetPresentation(files: Record<string, Uint8Array>, sheetPaths: Map<string, string>, valueGridSize = 0) {
  let remainingGrid = 1000000 - valueGridSize;
  let styles: SheetStyle[] = [{}];
  try {
    let theme: (string | undefined)[] = [];
    try { theme = themeColors(files["xl/theme/theme1.xml"]); } catch { /* Unknown theme uses default colours. */ }
    styles = parseStyles(files["xl/styles.xml"], theme);
  } catch { /* Excessive/unsupported styles use the plain sheet font and grid. */ }
  return (sheet: Sheet): Sheet => {
    const bytes = files[sheetPaths.get(sheet.name) ?? ""];
    if (!bytes) return sheet;
    try {
      const metadata = parseSheet(bytes, styles);
      const { layout } = metadata;
      const rows = sheet.rows.length;
      const columns = sheet.rows[0]?.length ?? 0;
      // The value reader drops trailing empty cells, including styled table edges.
      const rowCount = Math.max(rows, metadata.rowCount);
      const columnCount = Math.max(columns, metadata.columnCount);
      const extra = rowCount * columnCount - rows * columns;
      if (extra > remainingGrid) return sheet;
      remainingGrid -= extra;
      for (const key of Object.keys(layout.cellStyles)) {
        const index = Number(key);
        if (Math.floor(index / COLUMN_LIMIT) >= rowCount || index % COLUMN_LIMIT >= columnCount)
          delete layout.cellStyles[index];
      }
      return {
        ...sheet,
        truncated: sheet.truncated || metadata.truncated,
        rows: Array.from({ length: rowCount }, (_, r) => Array.from({ length: columnCount }, (_, c) => sheet.rows[r]?.[c] ?? null)),
        layout: { ...layout, columnWidths: layout.columnWidths.slice(0, columnCount), rowHeights: layout.rowHeights.slice(0, rowCount) },
      };
    } catch { return sheet; }
  };
}
