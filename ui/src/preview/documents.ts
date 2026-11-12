import { zipSync, strToU8 } from "fflate";

const zip = (files: Record<string, string | Uint8Array>) => {
  const data = zipSync(
    Object.fromEntries(
      Object.entries(files).map(([key, value]) => [
        key,
        typeof value === "string" ? strToU8(value) : value,
      ]),
    ),
  );
  return data.buffer as ArrayBuffer;
};
const sheetNs =
  'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"';
const relNs =
  'xmlns="http://schemas.openxmlformats.org/package/2006/relationships"';
const text = (ref: string, value: string) =>
  `<c r="${ref}" t="inlineStr"><is><t>${value}</t></is></c>`;

/** Tiny self-contained Office archives, invented content only. */
export function spreadsheetFixture(large = false) {
  return zip({
    "[Content_Types].xml":
      '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>',
    "xl/workbook.xml": `<workbook ${sheetNs} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/><sheet name="Empty" sheetId="3" r:id="rId3"/></sheets></workbook>`,
    "xl/_rels/workbook.xml.rels": `<Relationships ${relNs}><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`,
    "xl/styles.xml": `<styleSheet ${sheetNs}><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>`,
    "xl/worksheets/sheet1.xml": `<worksheet ${sheetNs}><dimension ref="A1:${large ? "CW1001" : "E3"}"/><sheetData><row r="1">${text("A1", "Item")}${text("B1", "Amount")}${text("C1", "Date")}${text("D1", "Cached formula")}</row><row r="2">${text("A2", "Synthetic paper")}<c r="B2"><v>1234.5</v></c><c r="C2" s="1"><v>46000</v></c><c r="D2"><f>B2*2</f><v>2469</v></c><c r="E2" t="b"><v>1</v></c></row>${large ? '<row r="1000">' + text("A1000", "Last visible row") + text("CW1000", "Truncated column") + '</row><row r="1001">' + text("A1001", "Truncated row") + "</row>" : ""}</sheetData></worksheet>`,
    "xl/worksheets/sheet2.xml": `<worksheet ${sheetNs}><sheetData><row r="1">${text("A1", "Values only — no formula execution")}</row></sheetData></worksheet>`,
    "xl/worksheets/sheet3.xml": `<worksheet ${sheetNs}><sheetData/></worksheet>`,
  });
}

export function wordFixture() {
  const ns =
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
  return zip({
    "[Content_Types].xml":
      '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/></Types>',
    "_rels/.rels": `<Relationships ${relNs}><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>`,
    "word/styles.xml": `<w:styles ${ns}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>`,
    "word/numbering.xml": `<w:numbering ${ns}><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>`,
    "word/_rels/document.xml.rels": `<Relationships ${relNs}><Relationship Id="rImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pixel.png"/><Relationship Id="rLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="javascript:alert(1)" TargetMode="External"/></Relationships>`,
    "word/media/pixel.png": Uint8Array.from(
      atob(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aJ1sAAAAASUVORK5CYII=",
      ),
      (c) => c.charCodeAt(0),
    ),
    "word/document.xml": `<w:document ${ns} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Synthetic document</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/><w:i/><w:u w:val="single"/></w:rPr><w:t>Bold, italic and underlined</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>List item</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Table cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:hyperlink r:id="rLink"><w:r><w:t>Inert link text</w:t></w:r></w:hyperlink></w:p><w:p><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p><w:sectPr/></w:body></w:document>`,
  });
}
