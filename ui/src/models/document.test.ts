import assert from "node:assert/strict";
import test from "node:test";
import { JSDOM } from "jsdom";
import readExcel from "read-excel-file/universal";
import mammoth from "mammoth";
import { zipSync, unzipSync, strToU8 } from "fflate";
import {
  DOCUMENT_MIMES,
  canView,
  documentKind,
  cellFormatter,
  columnLetter,
  limitSheet,
  ROW_LIMIT,
  COLUMN_LIMIT,
  HTML_LIMIT,
} from "./document.ts";
import { sanitizeDocument } from "./document-sanitize.ts";
import { boundedArchive, ENTRY_LIMIT } from "./document-archive.ts";
import { spreadsheetFixture, wordFixture } from "../preview/documents.ts";

const dom = new JSDOM("");
const sanitize = (html: string) =>
  sanitizeDocument(html, dom.window as unknown as Window & typeof globalThis);

test("viewer requires exact document filename/MIME, downloaded and approved", () => {
  for (const [kind, mime] of Object.entries(DOCUMENT_MIMES)) {
    const file = {
      name: `sample.${kind.toUpperCase()}`,
      mimetype: mime,
      local_path: "/synthetic/file",
      can_open: true,
      hidden_by_limit: false,
    };
    assert.equal(documentKind(file), kind);
    assert.equal(canView(file), true);
    for (const patch of [
      { name: "sample.xls" },
      { name: "sample.doc" },
      { name: "sample.pptx" },
      { name: "sample.ods" },
      { mimetype: "text/html" },
      { local_path: null },
      { can_open: false },
      { can_open: undefined },
      { hidden_by_limit: true },
    ])
      assert.equal(canView({ ...file, ...patch }), false);
  }
  assert.equal(
    canView({
      name: "sample.svg",
      mimetype: "image/svg+xml",
      local_path: "/synthetic/file",
      can_open: true,
      hidden_by_limit: false,
    }),
    false,
  );
});

test("sanitizer removes active HTML, links, CSS, clobbering and external/SVG images", () => {
  const clean = sanitize(
    '<script>alert(1)</script><p id="document" style="background:url(https://example.test)" onclick="alert(1)">Safe <b>bold</b></p><a href="javascript:alert(1)">inert</a><a href="https://example.test">web</a><iframe src="https://example.test"></iframe><form><input><button>Submit</button></form><object/><embed/><base href="https://example.test"><link rel="stylesheet"><meta http-equiv="refresh"><img src="https://example.test/i.png" onerror="alert(1)"><img src="data:image/svg+xml;base64,PHN2Zy8+"><svg onload="alert(1)"></svg>',
  );
  assert.match(clean, /<p>Safe <b>bold<\/b><\/p>/);
  assert.match(clean, /inert/);
  assert.doesNotMatch(
    clean,
    /<(script|a\b|iframe|form|input|button|object|embed|base|link|meta|svg|img)|onerror|onclick|style=|id=|javascript:|https:/,
  );
});

test("sanitizer preserves readable formatting and raster data images only", () => {
  const html =
    '<h1>Heading</h1><p><strong>B</strong><em>I</em><u>U</u><br></p><ul><li>Item</li></ul><ol><li>One</li></ol><table><tr><td colspan="2">Cell</td></tr></table><img alt="Pixel" src="data:image/png;base64,YQ==">';
  const clean = sanitize(html);
  for (const tag of [
    "h1",
    "p",
    "strong",
    "em",
    "u",
    "ul",
    "li",
    "ol",
    "table",
    "td",
    "img",
  ])
    assert.match(clean, new RegExp(`<${tag}[ >]`));
  assert.match(clean, /colspan="2"/);
  assert.match(clean, /data:image\/png;base64,YQ==/);
  assert.doesNotMatch(
    sanitize('<table><tr><td colspan="999999">x</td></tr></table>'),
    /colspan/,
  );
  assert.throws(() => sanitize("x".repeat(HTML_LIMIT + 1)), /document_limit/);
  assert.throws(() => sanitize("<p>x</p>".repeat(20001)), /document_limit/);
});

test("sheet bounds and column letters", () => {
  const rows = Array.from({ length: ROW_LIMIT + 1 }, () =>
    Array(COLUMN_LIMIT + 1).fill("x"),
  );
  const limited = limitSheet("Test", rows);
  assert.equal(limited.truncated, true);
  assert.equal(limited.rows.length, ROW_LIMIT);
  assert.equal(limited.rows[0].length, COLUMN_LIMIT);
  assert.equal(limitSheet("Empty", []).truncated, false);
  assert.deepEqual(limitSheet("Sparse", [["a"], [null, 2]]).rows, [
    ["a", null],
    [null, 2],
  ]);
  assert.deepEqual([0, 25, 26, 99].map(columnLetter), ["A", "Z", "AA", "CV"]);
});

test("cell values use locale, calendar dates and plain text", () => {
  const en = cellFormatter("en-US", "True", "False");
  const uk = cellFormatter("uk-UA", "Так", "Ні");
  assert.equal(en(1234.5), "1,234.5");
  assert.equal(uk(1234.5), new Intl.NumberFormat("uk-UA").format(1234.5));
  assert.equal(en(new Date("2026-11-12T00:00:00Z")), "Nov 12, 2026");
  assert.equal(en(null), "");
  assert.equal(en(NaN), "");
  assert.equal(en(new Date(NaN)), "");
  assert.equal(uk(true), "Так");
  assert.equal(uk(false), "Ні");
  assert.equal(en("<script>literal</script>"), "<script>literal</script>");
});

test("real synthetic XLSX parsing preserves sheets, dates and cached formula values", async () => {
  const archive = boundedArchive(spreadsheetFixture(true), true);
  assert.deepEqual([...archive.truncatedSheets], ["Budget"]);
  const sheets = await readExcel(archive.data, { trim: false });
  assert.deepEqual(
    sheets.map((s) => s.sheet),
    ["Budget", "Notes", "Empty"],
  );
  assert.equal(sheets[0].data.length, ROW_LIMIT);
  assert.equal(sheets[0].data[1][1], 1234.5);
  assert.ok(sheets[0].data[1][2] instanceof Date);
  assert.equal(sheets[0].data[1][3], 2469);
  assert.equal(sheets[0].data[1][4], true);
  assert.equal(sheets[0].data[999][0], "Last visible row");
  assert.ok(sheets[0].data.every((row) => row.length <= COLUMN_LIMIT));
  assert.deepEqual(sheets[2].data, []);
});

test("real synthetic DOCX conversion survives sanitization with image and list", async () => {
  const archive = boundedArchive(wordFixture(), false);
  const result = await mammoth.convertToHtml(
    { buffer: Buffer.from(archive.data) },
    {
      externalFileAccess: false,
      includeEmbeddedStyleMap: false,
      styleMap: ["u => u"],
    },
  );
  const html = sanitize(result.value);
  assert.match(html, /<h1>Synthetic document<\/h1>/);
  for (const tag of ["strong", "em", "u", "ul", "li", "table", "td", "img"])
    assert.match(html, new RegExp(`<${tag}[ >]`));
  assert.match(html, /data:image\/png;base64,/);
  assert.match(html, /Inert link text/);
  assert.doesNotMatch(html, /javascript:|href=/);
});

test("archive rejects broken, encrypted, traversal, oversized and deeply nested inputs", () => {
  const archive = (files: Record<string, Uint8Array>) =>
    zipSync(files).buffer as ArrayBuffer;
  for (const data of [
    new ArrayBuffer(0),
    strToU8("broken").buffer,
    new Uint8Array([0xd0, 0xcf, 0x11, 0xe0]).buffer,
  ])
    assert.throws(() => boundedArchive(data as ArrayBuffer, false));
  assert.throws(
    () => boundedArchive(archive({ "../escape.xml": strToU8("<a/>") }), false),
    /document_limit/,
  );
  assert.throws(
    () =>
      boundedArchive(
        archive({ "large.xml": new Uint8Array(ENTRY_LIMIT + 1) }),
        false,
      ),
    /document_limit/,
  );
  assert.throws(
    () =>
      boundedArchive(
        archive({
          "deep.xml": strToU8("<a>".repeat(129) + "</a>".repeat(129)),
        }),
        false,
      ),
    /document_limit/,
  );
  assert.throws(
    () =>
      boundedArchive(
        archive({
          "entity.xml": strToU8('<!DOCTYPE a [<!ENTITY x "boom">]><a>&x;</a>'),
        }),
        false,
      ),
    /document_limit/,
  );
  assert.throws(
    () =>
      boundedArchive(
        archive(
          Object.fromEntries(
            Array.from({ length: 513 }, (_, i) => [
              `${i}.xml`,
              strToU8("<a/>"),
            ]),
          ),
        ),
        false,
      ),
    /document_limit/,
  );
  // Lie about uncompressed size in both headers: actual streaming count still wins.
  const forged = new Uint8Array(
    archive({ "bomb.xml": new Uint8Array(ENTRY_LIMIT + 1) }),
  );
  const view = new DataView(forged.buffer);
  view.setUint32(22, 1, true);
  for (let i = 0; i < forged.length - 24; i++)
    if (view.getUint32(i, true) === 0x02014b50) view.setUint32(i + 24, 1, true);
  assert.throws(() => boundedArchive(forged.buffer, false), /document_limit/);
});

test("sparse hostile cell references cannot cause huge row/column allocations", () => {
  const files = unzipSync(new Uint8Array(spreadsheetFixture()));
  files["xl/worksheets/sheet1.xml"] = strToU8(
    '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1048576"><c r="XFD1048576"><v>1</v></c></row></sheetData></worksheet>',
  );
  const result = boundedArchive(zipSync(files).buffer as ArrayBuffer, true);
  assert.equal(result.truncatedSheets.has("Budget"), true);
  const safe = unzipSync(new Uint8Array(result.data));
  assert.doesNotMatch(
    new TextDecoder().decode(safe["xl/worksheets/sheet1.xml"]),
    /1048576|XFD/,
  );
});

test("worker client terminates after success, failure, close and deadline", async (context) => {
  const { parseOffice } = await import("./document-client.ts");
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  const instances: FakeWorker[] = [];
  class FakeWorker {
    terminated = false;
    onmessage?: (event: { data: unknown }) => void;
    onerror?: (event: { preventDefault: () => void }) => void;
    constructor() {
      instances.push(this);
    }
    postMessage() {}
    terminate() {
      this.terminated = true;
    }
  }
  Object.defineProperty(globalThis, "Worker", {
    configurable: true,
    value: FakeWorker,
  });
  context.after(() => {
    if (original) Object.defineProperty(globalThis, "Worker", original);
    else delete (globalThis as { Worker?: unknown }).Worker;
  });
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const success = parseOffice(
    new ArrayBuffer(0),
    "xlsx",
    new AbortController().signal,
  );
  instances.at(-1)!.onmessage!({
    data: { result: { kind: "xlsx", sheets: [] } },
  });
  assert.deepEqual(await success, { kind: "xlsx", sheets: [] });
  assert.equal(instances.at(-1)!.terminated, true);

  const closed = new AbortController();
  const closing = parseOffice(new ArrayBuffer(0), "docx", closed.signal);
  const closedResult = assert.rejects(closing, /closed/);
  closed.abort();
  await closedResult;
  assert.equal(instances.at(-1)!.terminated, true);

  const timed = parseOffice(
    new ArrayBuffer(0),
    "docx",
    new AbortController().signal,
  );
  const timedResult = assert.rejects(timed, /document_limit/);
  context.mock.timers.tick(15000);
  await timedResult;
  assert.equal(instances.at(-1)!.terminated, true);

  const failed = parseOffice(
    new ArrayBuffer(0),
    "docx",
    new AbortController().signal,
  );
  const failedResult = assert.rejects(failed, /invalid_document/);
  instances.at(-1)!.onerror!({ preventDefault() {} });
  await failedResult;
  assert.equal(instances.at(-1)!.terminated, true);
});

test("namespace aliases and unusual roots cannot bypass sparse worksheet limits", async () => {
  const files = unzipSync(new Uint8Array(spreadsheetFixture()));
  files["xl/worksheets/sheet1.xml"] = strToU8(
    '<odd-root xmlns:p="urn:example"><p:sheetData><p:row p:r="1048576"><p:c p:r="XFD1048576"><p:v>1</p:v></p:c></p:row></p:sheetData></odd-root>',
  );
  const result = boundedArchive(zipSync(files).buffer as ArrayBuffer, true);
  assert.deepEqual([...result.truncatedSheets], ["Budget"]);
  const sheets = await readExcel(result.data);
  assert.deepEqual(sheets[0].data, []);
  files["xl/worksheets/sheet1.xml"] = strToU8(
    '<worksheet xmlns:p="urn:example"><sheetData><row r="1" p:r="1048576"/></sheetData></worksheet>',
  );
  assert.throws(
    () => boundedArchive(zipSync(files).buffer as ArrayBuffer, true),
    /document_limit/,
  );
});
