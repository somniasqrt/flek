// Launch eligibility is supplied by core. Missing/older IPC data fails closed.
export type FilePresentation = { name: string | null; can_open?: boolean };
export const canOpen = (file: FilePresentation) => file.can_open === true;
export const fileName = (file: FilePresentation, fallback: string) =>
  file.name?.replace(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "") || fallback;

// Active image documents (notably SVG) never enter inline previews.
export const isRasterImage = (mime: string | null | undefined) =>
  /^(image\/(png|jpeg|gif|webp|avif|bmp|x-icon|vnd.microsoft.icon))$/i.test(mime ?? "");

/** Same narrow media types as flekfile; core still gates the resolved cache path. */
export function isInlineVideo(file: { mimetype: string | null; local_path: string | null; can_open?: boolean; hidden_by_limit: boolean }) {
  return !file.hidden_by_limit && !!file.local_path && file.can_open === true &&
    ["video/mp4", "video/x-m4v", "video/quicktime", "video/webm", "video/ogg"].includes(file.mimetype ?? "");
}
