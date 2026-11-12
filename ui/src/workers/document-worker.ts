import { boundedArchive } from "../models/document-archive";
import {
  limitSheet,
  cellValue,
  HTML_LIMIT,
  type OfficeResult,
} from "../models/document";

self.onmessage = async (
  event: MessageEvent<{ data: ArrayBuffer; kind: "xlsx" | "docx" }>,
) => {
  try {
    const kind = event.data.kind;
    const archive = boundedArchive(event.data.data, kind === "xlsx");
    let result: OfficeResult;
    if (kind === "xlsx") {
      const { default: read } = await import("read-excel-file/universal");
      const sheets = await read(archive.data, { trim: false });
      if (!sheets.length || sheets.length > 100)
        throw new Error("document_limit");
      result = {
        kind,
        sheets: sheets.map((s) =>
          limitSheet(
            s.sheet,
            s.data.map((row) => row.map(cellValue)),
            archive.truncatedSheets.has(s.sheet),
          ),
        ),
      };
    } else {
      const { default: mammoth } = await import("mammoth");
      const converted = await mammoth.convertToHtml(
        { arrayBuffer: archive.data },
        {
          externalFileAccess: false,
          includeEmbeddedStyleMap: false,
          styleMap: ["u => u"],
          convertImage: mammoth.images.imgElement(async (image) => {
            if (
              !/^image\/(png|jpeg|gif|webp|avif|bmp)$/.test(image.contentType)
            )
              return { src: "" };
            return {
              src: `data:${image.contentType};base64,${await image.readAsBase64String()}`,
            };
          }),
        },
      );
      if (converted.value.length > HTML_LIMIT)
        throw new Error("document_limit");
      result = { kind, html: converted.value };
    }
    self.postMessage({ result });
  } catch (error) {
    // Never send parser messages containing untrusted document contents/paths.
    self.postMessage({
      error:
        error instanceof Error && error.message === "document_limit"
          ? "limit"
          : "invalid",
    });
  }
};
