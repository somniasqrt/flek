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
const text = (ref: string, value: string, style = 0) =>
  `<c r="${ref}" s="${style}" t="inlineStr"><is><t>${value}</t></is></c>`;

/** Tiny self-contained Office archives, invented content only. */
export function spreadsheetFixture(large = false) {
  return zip({
    "[Content_Types].xml":
      '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>',
    "xl/workbook.xml": `<workbook ${sheetNs} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/><sheet name="Empty" sheetId="3" r:id="rId3"/></sheets></workbook>`,
    "xl/_rels/workbook.xml.rels": `<Relationships ${relNs}><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>`,
    "xl/styles.xml": `<styleSheet ${sheetNs}><fonts count="3"><font><sz val="11"/><color rgb="FF202124"/></font><font><b/><sz val="11"/><color rgb="FFFFFFFF"/></font><font><b/><i/><u/><sz val="18"/><color theme="4" tint="-0.25"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor theme="4" tint="0.2"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color indexed="23"/></left><right style="thin"><color indexed="23"/></right><top style="thin"><color indexed="23"/></top><bottom style="thin"><color indexed="23"/></bottom></border></borders><cellXfs count="6"><xf numFmtId="0" fontId="0"/><xf numFmtId="14" fontId="0" borderId="1"/><xf numFmtId="0" fontId="1" fillId="2" borderId="1"><alignment horizontal="center" vertical="center"/></xf><xf numFmtId="0" fontId="0" borderId="1"/><xf numFmtId="0" fontId="2"><alignment vertical="center"/></xf><xf numFmtId="0" fontId="0" borderId="1"><alignment wrapText="1" vertical="top"/></xf></cellXfs></styleSheet>`,
    "xl/theme/theme1.xml": '<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Synthetic"><a:dk1><a:srgbClr val="202124"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="25638A"/></a:accent1></a:clrScheme></a:themeElements></a:theme>',
    "xl/worksheets/sheet1.xml": `<worksheet ${sheetNs}><dimension ref="A1:${large ? "CW1001" : "P9"}"/><sheetFormatPr defaultColWidth="8.43" defaultRowHeight="15"/><cols><col min="1" max="1" width="14"/><col min="2" max="2" width="9"/><col min="3" max="3" width="12"/><col min="4" max="4" width="14"/><col min="5" max="5" width="5"/><col min="6" max="6" hidden="1"/><col min="7" max="16" width="7"/></cols><sheetData><row r="1" ht="22">${text("A1", "Item", 2)}${text("B1", "Amount", 2)}${text("C1", "Date", 2)}${text("D1", "Cached formula", 2)}${text("J1", "Team", 2)}${text("K1", "Plan", 2)}${text("L1", "Actual", 2)}</row><row r="2">${text("A2", "Synthetic paper", 3)}<c r="B2" s="3"><v>1234.5</v></c><c r="C2" s="1"><v>46000</v></c><c r="D2" s="3"><f>B2*2</f><v>2469</v></c><c r="E2" t="b"><v>1</v></c>${text("J2", "North", 3)}<c r="K2" s="3"><v>18</v></c><c r="L2" s="3"><v>16</v></c></row><row r="3">${text("J3", "South", 3)}<c r="K3" s="3"><v>12</v></c><c r="L3" s="3"><v>14</v></c></row><row r="4" ht="24">${text("A4", "Synthetic budget — merged title", 4)}</row><row r="5" ht="14"/><row r="6" hidden="1">${text("A6", "Hidden row")}</row><row r="7" ht="48">${text("A7", "Wrapped text stays inside this merged cell and uses the saved row height.", 5)}</row><row r="9">${text("A9", "This text spills across empty cells until the next saved value.")}${text("H9", "Stops here")}${text("P9", "100%")}</row>${large ? '<row r="1000">' + text("A1000", "Last visible row") + text("CV1000", "Last visible column") + text("CW1000", "Truncated column") + '</row><row r="1001">' + text("A1001", "Truncated row") + "</row>" : ""}</sheetData><mergeCells count="2"><mergeCell ref="A4:H5"/><mergeCell ref="A7:C7"/></mergeCells></worksheet>`,
    "xl/worksheets/sheet2.xml": `<worksheet ${sheetNs}><sheetFormatPr defaultColWidth="8.43" defaultRowHeight="15"/><sheetData><row r="1">${text("A1", "Values only — no formula execution")}${text("J1", "End of spill")}</row></sheetData></worksheet>`,
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
