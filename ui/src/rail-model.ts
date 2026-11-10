import { t } from "./i18n/index.ts";
import { readable } from "./snippet.ts";

export type RailView = "home" | "dms" | "activity" | "files";
export const railViews: RailView[] = ["home", "dms", "activity", "files"];

export function messagePreview(
  row: {
    text: string;
    user_id: string | null;
    file_name: string | null;
    hidden_by_limit: boolean;
  },
  me: string,
  people: Record<string, string>,
  emoji?: (name: string) => string | undefined,
): string {
  let text = readable(row.text, people, emoji);
  if (!text) {
    text = row.hidden_by_limit
      ? t("Hidden by Slack's 90-day limit")
      : row.file_name ?? t("No messages yet");
  }
  return row.user_id === me ? t("You: {text}", { text }) : text;
}

export function fileType(mimetype: string | null): "image" | "pdf" | "doc" | "other" {
  const mime = mimetype ?? "";
  if (mime.startsWith("image/")) return "image";
  if (mime === "application/pdf") return "pdf";
  if (
    mime.startsWith("text/") ||
    /word|document|excel|spreadsheet|presentation|powerpoint/.test(mime)
  ) {
    return "doc";
  }
  return "other";
}

export function readRail(key: string): RailView {
  try {
    const view = localStorage.getItem(key) as RailView;
    return railViews.includes(view) ? view : "home";
  } catch {
    return "home";
  }
}
