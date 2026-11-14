import assert from "node:assert/strict";
import test from "node:test";
import { zipSync, strToU8 } from "fflate";
import { boundedArchive } from "./document-archive.ts";
import { COLUMN_LIMIT, type Sheet } from "./document.ts";
import { MERGE_LIMIT, STYLE_LIMIT, spreadsheetPresentation } from "./spreadsheet-layout.ts";
import { spreadsheetFixture } from "../preview/documents.ts";

const ns = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"';
const workbook = (sheet: string, styles?: string) => {
  const files: Record<string, string> = {
    "[Content_Types].xml":
      '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>',
    "xl/workbook.xml": `<workbook ${ns} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>`,
    "xl/_rels/workbook.xml.rels":
      '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>',
    "xl/worksheets/sheet1.xml": `<worksheet ${ns}>${sheet}</worksheet>`,
  };
  if (styles) files["xl/styles.xml"] = `<styleSheet ${ns}>${styles}</styleSheet>`;
  const data = zipSync(Object.fromEntries(Object.entries(files).map(([k, v]) => [k, strToU8(v)])));
  return data.buffer as ArrayBuffer;
};
const present = (data: ArrayBuffer, rows: Sheet["rows"], name = "S") => {
  const archive = boundedArchive(data, true);
  return spreadsheetPresentation(archive.files, archive.sheetPaths)({ name, rows, truncated: false });
};
const styleAt = (sheet: Sheet, row: number, column: number) =>
  sheet.layout!.styles[sheet.layout!.cellStyles[row * COLUMN_LIMIT + column] ?? 0];

test("sheet layout comes from the file: widths, heights, hidden parts and merges", () => {
  const sheet = present(
    workbook(
      '<sheetFormatPr defaultColWidth="8.43" defaultRowHeight="15"/>' +
        '<cols><col min="1" max="1" width="14"/><col min="3" max="3" hidden="1"/><col min="4" max="5" width="0.5"/></cols>' +
        '<sheetData><row r="1" ht="30"><c r="A1" t="inlineStr"><is><t>Title</t></is></c></row>' +
        '<row r="2" hidden="1"><c r="B2"><v>1</v></c></row><row r="3"><c r="E3"><v>2</v></c></row></sheetData>' +
        '<mergeCells><mergeCell ref="A1:D1"/></mergeCells>',
    ),
    [["Title"], [null, 1], [null, null, null, null, 2]],
  );
  const layout = sheet.layout!;
  // Excel character units: round(width × 7) + 5 px; under one character × 12.
  assert.deepEqual(layout.columnWidths, [103, 64, 0, 6, 6]);
  assert.deepEqual(layout.rowHeights, [40, 0, 20]);
  assert.deepEqual(layout.merges, [{ row: 0, column: 0, endRow: 0, endColumn: 3 }]);
  // Trailing empty cells the value reader dropped are restored to a full grid.
  assert.deepEqual(sheet.rows.map((r) => r.length), [5, 5, 5]);
  assert.equal(sheet.rows[2][4], 2);
  assert.equal(sheet.truncated, false);
});

test("cell styles reach the UI only as validated values", () => {
  const sheet = present(
    workbook(
      '<sheetData><row r="1"><c r="A1" s="1" t="inlineStr"><is><t>a</t></is></c><c r="B1" s="2" t="inlineStr"><is><t>b</t></is></c><c r="C1" s="99"><v>1</v></c></row></sheetData>',
      '<fonts><font><sz val="11"/></font><font><b/><i/><u/><sz val="18"/><color rgb="FF112233"/></font>' +
        '<font><b val="0"/><color rgb="red;background:url(https://example.test)"/></font></fonts>' +
        '<fills><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>' +
        '<fill><patternFill patternType="solid"><fgColor rgb="FFABCDEF"/></patternFill></fill>' +
        '<fill><patternFill patternType="solid"><fgColor rgb="javascript:alert(1)"/></patternFill></fill></fills>' +
        '<borders><border/><border><left style="thick"><color indexed="2"/></left><right style="dashed"/><top style="expression(1)"/><bottom style="double"><color rgb="FF00FF00"/></bottom></border></borders>' +
        '<cellXfs><xf/><xf fontId="1" fillId="2" borderId="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>' +
        '<xf fontId="2" fillId="3"><alignment horizontal="url(x)" vertical="evil"/></xf></cellXfs>',
    ),
    [["a", "b", 1]],
  );
  assert.deepEqual(styleAt(sheet, 0, 0), {
    bold: true,
    italic: true,
    underline: true,
    fontSize: 24,
    color: "#112233",
    fill: "#abcdef",
    left: { style: "solid", width: 3, color: "#ff0000" },
    right: { style: "dashed", width: 1, color: "#000000" },
    top: undefined,
    bottom: { style: "double", width: 3, color: "#00ff00" },
    horizontal: "center",
    vertical: "middle",
    wrap: true,
  });
  const hostile = styleAt(sheet, 0, 1);
  assert.equal(hostile.bold, false);
  for (const key of ["color", "fill", "horizontal", "vertical"] as const)
    assert.equal(hostile[key], undefined, key);
  // An unknown style index falls back to the default style.
  assert.deepEqual(styleAt(sheet, 0, 2), sheet.layout!.styles[0]);
  // Nothing from the file survives except hex colours, numbers and keywords.
  assert.doesNotMatch(JSON.stringify(sheet.layout), /url|javascript|expression|evil|example/);
});

test("excessive or inconsistent metadata falls back to the plain sheet, keeping values", () => {
  const rows: Sheet["rows"] = [["kept"]];
  const data = '<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>kept</t></is></c></row></sheetData>';
  const merges = (n: number) =>
    `<mergeCells>${Array.from({ length: n }, (_, i) => `<mergeCell ref="A${2 * i + 2}:B${2 * i + 2}"/>`).join("")}</mergeCells>`;
  assert.equal(present(workbook(data + merges(400)), rows).layout!.merges.length, 400);
  for (const extra of [
    merges(MERGE_LIMIT + 1),
    '<mergeCells><mergeCell ref="A1:B2"/><mergeCell ref="B2:C3"/></mergeCells>',
  ]) {
    const sheet = present(workbook(data + extra), rows);
    assert.equal(sheet.layout, undefined);
    assert.deepEqual(sheet.rows, rows);
  }
  // Too many styles: layout stays, styling does not.
  const styled = present(
    workbook(
      '<sheetData><row r="1"><c r="A1" s="1" t="inlineStr"><is><t>kept</t></is></c></row></sheetData>',
      `<fonts><font/><font><b/></font></fonts><cellXfs>${"<xf fontId=\"1\"/>".repeat(STYLE_LIMIT + 1)}</cellXfs>`,
    ),
    rows,
  );
  assert.deepEqual(styled.layout!.styles, [{}]);
  assert.deepEqual(styled.rows, rows);
  // A merge that leaves the preview window is clipped and reported.
  const clipped = present(workbook(data + '<mergeCells><mergeCell ref="A1:A5000"/></mergeCells>'), rows);
  assert.equal(clipped.truncated, true);
  assert.equal(clipped.layout!.merges[0].endRow, 999);
  // A sheet the archive does not know is returned untouched.
  assert.equal(present(workbook(data), rows, "Other").layout, undefined);
});

test("preview workbook carries widths, a hidden column, a merged title and themed styles", () => {
  const archive = boundedArchive(spreadsheetFixture(), true);
  const sheet = spreadsheetPresentation(archive.files, archive.sheetPaths)({
    name: "Budget",
    rows: [["Item"]],
    truncated: false,
  });
  const layout = sheet.layout!;
  assert.equal(layout.columnWidths[0], 103);
  assert.equal(layout.columnWidths[5], 0);
  assert.equal(layout.rowHeights[5], 0);
  assert.ok(layout.merges.some((m) => m.row === 3 && m.column === 0 && m.endRow === 4 && m.endColumn === 7));
  const header = styleAt(sheet, 0, 0);
  assert.equal(header.bold, true);
  assert.equal(header.color, "#ffffff");
  assert.match(header.fill!, /^#[0-9a-f]{6}$/);
  assert.equal(header.left!.color, "#808080");
});
