import type { Message } from "../api";
import { parseBlockKit } from "./block-kit.ts";

export function slackDeepLink(team: string, conversation: string, ts?: string): string {
  return (
    `slack://channel?team=${encodeURIComponent(team)}&id=${encodeURIComponent(conversation)}` +
    (ts ? `&message=${encodeURIComponent(ts)}` : "")
  );
}
export function needsSlack(
  message: Pick<Message, "blocks" | "subtype" | "files" | "unsupported">,
): boolean {
  return (
    !!message.unsupported ||
    parseBlockKit(message.blocks).fallback ||
    ["huddle_thread", "canvas", "slack_call"].includes(message.subtype ?? "") ||
    message.files.some(
      (f) => f.filetype === "canvas" || f.mimetype === "application/vnd.slack-docs",
    )
  );
}
