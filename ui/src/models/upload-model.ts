// Keep these bounded staging limits in sync with core::sync::upload.
export const MAX_UPLOAD_BYTES = 100 * 1024 * 1024;
export const MAX_UPLOAD_FILES = 10;
export const UPLOAD_CHUNK_BYTES = 256 * 1024;
export type UploadProgress = { stage: "staging" | "preparing" | "uploading" | "completing" | "syncing"; sent: number; total: number; file_id?: string | null };
export function uploadFileError(file: Pick<File, "name" | "size">): "upload_empty" | "upload_too_large" | "upload_bad_name" | null {
  if (!Number.isSafeInteger(file.size) || file.size <= 0) return "upload_empty";
  if (file.size > MAX_UPLOAD_BYTES) return "upload_too_large";
  if (!file.name || new TextEncoder().encode(file.name).length > 255 || /[\p{Cc}/\\]/u.test(file.name)) return "upload_bad_name";
  return null;
}
export function uploadPercent(progress: UploadProgress | undefined): number {
  return progress?.total ? Math.min(100, Math.max(0, Math.floor(progress.sent / progress.total * 100))) : 0;
}
export const uploadDisplayName = (name: string) => name.replace(/[\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
