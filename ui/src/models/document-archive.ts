import { Unzip, UnzipInflate, zipSync, strFromU8, strToU8 } from "fflate";
import { Parser } from "saxen";
import { COLUMN_LIMIT, ROW_LIMIT, DOCUMENT_LIMIT } from "./document.ts";

export const INFLATED_LIMIT = 32 * 1024 * 1024;
export const ENTRY_LIMIT = 8 * 1024 * 1024;
const FILE_LIMIT = 512;
const NODE_LIMIT = 200000;
const GRID_LIMIT = 1000000;
/// Too big or too complex for the viewer, though possibly a valid file.
const fail = () => {
  throw new Error("document_limit");
};
/// Malformed, hostile or unsupported structure.
const invalid = () => {
  throw new Error("invalid_document");
};
const escapeXml = (s: string) =>
  s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/"/g, "&quot;")
    .replace(/>/g, "&gt;");

/** Stream actual output counts; never trust ZIP size declarations. Repack only
 * validated entries so downstream ZIP readers cannot allocate from forged sizes. */
export function boundedArchive(data: ArrayBuffer, spreadsheet: boolean) {
  if (data.byteLength > DOCUMENT_LIMIT) fail();
  const signature = new Uint8Array(data, 0, Math.min(data.byteLength, 4));
  if (
    signature.length < 4 ||
    signature[0] !== 0x50 ||
    signature[1] !== 0x4b ||
    signature[2] !== 3 ||
    signature[3] !== 4
  )
    throw new Error("invalid_document");
  const files: Record<string, Uint8Array> = Object.create(null);
  let total = 0;
  let count = 0;
  let nodes = 0;
  const truncatedPaths = new Set<string>();
  const sheetNames = new Map<string, string>();
  const sheetTargets = new Map<string, string>();
  let gridSize = 0;
  const unzip = new Unzip((file) => {
    if (
      ++count > FILE_LIMIT ||
      (file.originalSize !== undefined && file.originalSize > ENTRY_LIMIT)
    )
      fail();
    if (
      Object.hasOwn(files, file.name) ||
      /(^\/|\\|(^|\/)\.\.?(\/|$))/.test(file.name)
    )
      invalid();
    files[file.name] = new Uint8Array();
    let size = 0;
    const parts: Uint8Array[] = [];
    file.ondata = (error, chunk, final) => {
      if (error) throw error;
      total += chunk.length;
      size += chunk.length;
      if (total > INFLATED_LIMIT || size > ENTRY_LIMIT) fail();
      parts.push(chunk);
      if (final) {
        const bytes = new Uint8Array(size);
        let offset = 0;
        for (const part of parts) {
          bytes.set(part, offset);
          offset += part.length;
        }
        files[file.name] = bytes;
      }
    };
    file.start();
  });
  unzip.register(UnzipInflate);
  const bytes = new Uint8Array(data);
  // Small compressed input slices also bound each inflater output allocation.
  for (let offset = 0; offset < bytes.length; offset += 256)
    unzip.push(
      bytes.subarray(offset, offset + 256),
      offset + 256 >= bytes.length,
    );
  if (!count) invalid();
  for (const [name, bytes] of Object.entries(files)) {
    if (!/\.(xml|rels)$/i.test(name)) continue;
    const xml = strFromU8(bytes);
    if (/<!DOCTYPE|<!ENTITY/i.test(xml)) invalid();
    // The reader strips namespace prefixes and accepts sheetData under any
    // root. Filter every spreadsheet XML entry with the same interpretation.
    const sheet = spreadsheet;
    const output: string[] = [];
    let depth = 0;
    let skipped = 0;
    let row = 0;
    let column = 0;
    let maxRow = 0;
    let maxColumn = 0;
    // Grid rules apply inside sheetData only: calcChain.xml also has <c r="…"/>
    // elements, outside any row. 0 = not inside sheetData.
    let dataDepth = 0;
    const parser = new Parser();
    parser.on("error", invalid);
    parser.on("warn", invalid);
    parser.on("openTag", (tag, attrs, decode, selfClosing) => {
      if (++nodes > NODE_LIMIT || ++depth > 128) fail();
      if (!sheet) return;
      if (skipped) {
        skipped++;
        return;
      }
      const local = tag.split(":").pop();
      const rawAttributes = attrs();
      const attributes: Record<string, string> = Object.create(null);
      for (const [key, value] of Object.entries(rawAttributes)) {
        if (key === "xmlns" || key.startsWith("xmlns:")) continue;
        const localKey = key.split(":").pop()!;
        if (Object.hasOwn(attributes, localKey)) invalid();
        attributes[localKey] = decode(value);
      }
      if (name === "xl/workbook.xml" && local === "sheet") {
        if (sheetNames.size >= 100) fail();
        sheetNames.set(attributes.name ?? "", attributes.id ?? "");
      }
      if (name === "xl/_rels/workbook.xml.rels" && local === "Relationship") {
        const path = new URL(
          attributes.Target ?? "",
          "https://office.invalid/xl/",
        ).pathname.slice(1);
        sheetTargets.set(attributes.Id ?? "", path);
      }
      let skip = local === "dimension";
      if (local === "sheetData" && !selfClosing) dataDepth = depth;
      if (local === "row" && dataDepth) {
        row = attributes.r === undefined ? row + 1 : Number(attributes.r);
        column = 0;
        if (!Number.isSafeInteger(row) || row < 1) invalid();
        if (row > ROW_LIMIT) {
          truncatedPaths.add(name);
          skip = true;
        }
      }
      if (local === "c" && dataDepth) {
        // `r` is optional; without it a cell follows the previous one.
        if (attributes.r === undefined) column++;
        else {
          const ref = attributes.r.match(/^([A-Z]{1,3})([1-9][0-9]{0,6})$/);
          if (!ref) invalid();
          column = [...ref![1]].reduce(
            (n, c) => n * 26 + c.charCodeAt(0) - 64,
            0,
          );
          if (Number(ref![2]) !== row) invalid();
        }
        if (column > COLUMN_LIMIT) {
          truncatedPaths.add(name);
          skip = true;
        }
      }
      if (skip) {
        skipped = 1;
        return;
      }
      if (local === "row" && dataDepth) maxRow = Math.max(maxRow, row);
      if (local === "c" && dataDepth) maxColumn = Math.max(maxColumn, column);
      output.push(
        `<${tag}${Object.entries(rawAttributes)
          .map(([key, value]) => ` ${key}="${escapeXml(decode(value))}"`)
          .join("")}${selfClosing ? "/" : ""}>`,
      );
    });
    parser.on("closeTag", (tag, _decode, selfClosing) => {
      if (depth === dataDepth) dataDepth = 0;
      depth--;
      if (skipped) {
        skipped--;
        return;
      }
      if (sheet && !selfClosing) output.push(`</${tag}>`);
    });
    parser.on("text", (text, decode) => {
      if (sheet && !skipped) output.push(escapeXml(decode(text)));
    });
    parser.on("cdata", (text) => {
      if (sheet && !skipped) output.push(escapeXml(text));
    });
    parser.parse(xml);
    if (depth !== 0) invalid();
    if (sheet) {
      gridSize += maxRow * maxColumn;
      if (gridSize > GRID_LIMIT) fail();
      files[name] = strToU8(output.join(""));
    }
  }
  // Stored entries avoid nested blob workers in the spreadsheet ZIP reader.
  const archive = zipSync(files, { level: 0 });
  const truncatedSheets = new Set(
    [...sheetNames]
      .filter(([, id]) => truncatedPaths.has(sheetTargets.get(id) ?? ""))
      .map(([name]) => name),
  );
  return { data: archive.buffer as ArrayBuffer, truncatedSheets };
}
