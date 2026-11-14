export const DOCUMENT_LIMIT = 50 * 1024 * 1024;
export const ROW_LIMIT = 1000;
export const COLUMN_LIMIT = 100;
export const HTML_LIMIT = 2 * 1024 * 1024;
export const DOCUMENT_MIMES = {
  pdf: "application/pdf",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
} as const;
export type DocumentKind = keyof typeof DOCUMENT_MIMES;
type ViewableFile = {
  name: string | null;
  mimetype: string | null;
  local_path: string | null;
  can_open?: boolean;
  viewer_kind?: "image" | "svg" | "video" | "pdf" | "xlsx" | "docx" | null;
  hidden_by_limit: boolean;
};

export function documentKind(
  file: Pick<ViewableFile, "name" | "mimetype">,
): DocumentKind | null {
  const ext = file.name?.split(".").pop()?.toLowerCase();
  const mime = file.mimetype?.split(";")[0].trim().toLowerCase();
  for (const kind of ["pdf", "xlsx", "docx"] as const) {
    if (ext === kind && mime === DOCUMENT_MIMES[kind]) return kind;
  }
  return null;
}

export const canView = (file: ViewableFile) =>
  !!file.local_path &&
  !file.hidden_by_limit &&
  file.viewer_kind != null;

/// Why the viewer cannot show a document: over the IPC byte cap, over the
/// parser limits (size, complexity, time), or unreadable.
export type ViewerFailure = "too_large" | "limit" | "invalid";
export function viewerFailure(error: unknown): ViewerFailure {
  const text = String(error);
  if (text.includes("document_too_large")) return "too_large";
  if (text.includes("document_limit")) return "limit";
  return "invalid";
}

export type CellValue = string | number | boolean | Date | null;
export type SheetBorder = {
  style: "solid" | "dashed" | "dotted" | "double";
  width: number;
  color: string;
};
export type SheetStyle = {
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  fontSize?: number;
  color?: string;
  fill?: string;
  left?: SheetBorder;
  right?: SheetBorder;
  top?: SheetBorder;
  bottom?: SheetBorder;
  horizontal?: "left" | "center" | "right" | "justify";
  vertical?: "top" | "middle" | "bottom";
  wrap?: boolean;
};
export type SheetMerge = {
  row: number;
  column: number;
  endRow: number;
  endColumn: number;
};
export type SheetLayout = {
  columnWidths: number[];
  rowHeights: number[];
  styles: SheetStyle[];
  // Sparse cell indexes use the fixed preview stride, not the sheet width.
  cellStyles: Record<number, number>;
  merges: SheetMerge[];
};
export type Sheet = {
  name: string;
  rows: CellValue[][];
  truncated: boolean;
  layout?: SheetLayout;
};
export type OfficeResult =
  | { kind: "xlsx"; sheets: Sheet[] }
  | { kind: "docx"; html: string };

export function cellValue(value: unknown): CellValue {
  return value instanceof Date ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
    ? value
    : null;
}

export function limitSheet(
  name: string,
  rows: CellValue[][],
  truncated = false,
): Sheet {
  const columns = Math.min(
    COLUMN_LIMIT,
    rows.reduce((n, row) => Math.max(n, row.length), 0),
  );
  return {
    name,
    rows: rows
      .slice(0, ROW_LIMIT)
      .map((row) => Array.from({ length: columns }, (_, i) => row[i] ?? null)),
    truncated:
      truncated ||
      rows.length > ROW_LIMIT ||
      rows.some((row) => row.length > COLUMN_LIMIT),
  };
}

export function columnLetter(index: number): string {
  let result = "";
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26))
    result = String.fromCharCode(65 + ((n - 1) % 26)) + result;
  return result;
}

export function cellFormatter(locale: string, yes: string, no: string) {
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: 12 });
  // Excel calendar dates are timezone-free; preserve the date decoded as UTC.
  const date = new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeZone: "UTC",
  });
  const dateTime = new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: "UTC",
  });
  return (value: CellValue): string => {
    if (value === null) return "";
    if (value instanceof Date) {
      if (!Number.isFinite(value.getTime())) return "";
      return (
        value.getUTCHours() || value.getUTCMinutes() || value.getUTCSeconds()
          ? dateTime
          : date
      ).format(value);
    }
    if (typeof value === "number")
      return Number.isFinite(value) ? number.format(value) : "";
    if (typeof value === "boolean") return value ? yes : no;
    return value;
  };
}
