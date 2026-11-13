import { overrideCacheUrl } from "../api";

// Cached images: a placeholder instead of flekfile:// (no app here).
const avatar =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="72" height="72"><rect width="72" height="72" fill="#e8a33d"/><circle cx="36" cy="28" r="14" fill="#fff"/><rect x="14" y="48" width="44" height="30" rx="15" fill="#fff"/></svg>',
  );
const placeholder =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800"><rect width="100%" height="100%" fill="#8fb3e8"/><text x="50%" y="50%" font-size="80" text-anchor="middle" fill="#fff" font-family="sans-serif">image</text></svg>',
  );
// A five-page PDF for the viewer (pdf.js rebuilds the missing xref table).
const pdfPage = (n: number) =>
  `${n} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Resources<</Font<</F1 9 0 R>>>>/Contents ${n + 10} 0 R>>endobj\n` +
  `${n + 10} 0 obj<</Length 60>>stream\nBT /F1 36 Tf 72 740 Td (Sample page ${n - 2}) Tj ET\nendstream endobj\n`;
const pdfPages = [3, 4, 5, 6, 7];
export const pdf =
  "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n" +
  `2 0 obj<</Type/Pages/Kids[${pdfPages.map((n) => `${n} 0 R`).join(" ")}]/Count ${pdfPages.length}>>endobj\n` +
  pdfPages.map(pdfPage).join("") +
  "9 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF";
const avatar2 = avatar.replace("%23e8a33d", "%235b8def");
const bigPhoto =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f2c14e"/><stop offset="1" stop-color="#d9822b"/></linearGradient></defs><rect width="512" height="512" fill="url(#g)"/><circle cx="256" cy="200" r="96" fill="#fff" opacity="0.92"/><rect x="96" y="330" width="320" height="260" rx="140" fill="#fff" opacity="0.92"/></svg>',
  );
// Custom emoji: a small party hat and a square badge.
const emojiParty =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><path d="M32 4 54 58H10Z" fill="#d94fa0"/><circle cx="32" cy="6" r="6" fill="#f5c400"/><path d="M20 34h24M15 46h34" stroke="#fff" stroke-width="5"/></svg>',
  );
const emojiBadge =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="4" y="4" width="56" height="56" rx="12" fill="#2f8f5b"/><path d="M18 33l10 10 18-22" stroke="#fff" stroke-width="7" fill="none"/></svg>',
  );
export const svgFixture = '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240"><rect width="320" height="240" fill="#3a76be"/><circle cx="160" cy="120" r="60" fill="#fff"/></svg>';
const cached: Record<string, string> = {
  "FJPEG": "/preview-image.jpg",
  "FJPEG_DOWNLOAD": "/preview-image.jpg",
  "FJPEG_DOWNLOAD/preview": "/preview-image.jpg",
  "FVIDEO": "/preview-video.mp4",
  "FVIDEO_DOWNLOAD": "/preview-video.mp4",
  "link/a1": avatar,
  "link/a4": avatar2,
  "link/e1": emojiParty,
  "link/e2": emojiBadge,
  "link/big1": bigPhoto,
};
export const installCache = () => overrideCacheUrl((path) => cached[path] ?? placeholder);
