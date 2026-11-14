import { t } from "../i18n/index";
import { api, type FileRef } from "../api";
import { openInSlack } from "../stores/slack";
import { fileActionLabel } from "./FileSafety";
import { attachmentAction, openAttachment } from "./file-actions";
import { downloadAttachment, downloading } from "./Download";
import { openObjectMenu, type ObjectAction } from "./ObjectMenu";
import { isContextKey } from "./ConversationMenu";
import type { Sender } from "./Viewer";

type MenuFile = Pick<FileRef, "id" | "name" | "can_open" | "viewer_kind" | "hidden_by_limit"> & { permalink?: string | null };
export function fileMenu(file: () => MenuFile, from: () => Sender | undefined = () => undefined) {
  const open = (e: MouseEvent | KeyboardEvent) => {
    const f = file();
    const sender = from();
    const busy = attachmentAction(f.id).busy || downloading(f.id);
    const items: ObjectAction[] = [];
    if (!f.hidden_by_limit) {
      items.push(
        { label: t("Open"), disabled: busy, run: () => openAttachment(f.id, sender) },
        { label: t("Download"), disabled: busy, run: () => downloadAttachment(f.id) },
        { label: fileActionLabel(f), disabled: busy, run: async () => { await api.fetchFile(f.id); await api.openFile(f.id); } },
      );
    }
    if (f.permalink) items.push({ label: t("Copy link"), run: () => navigator.clipboard.writeText(f.permalink!) });
    if (sender?.conversation_id) items.push({ label: t("Open in Slack"), run: () => openInSlack(sender.conversation_id!, sender.ts) });
    if (items.length) openObjectMenu(e, t("File actions"), items);
    else { e.preventDefault(); e.stopPropagation(); }
  };
  return {
    onContextMenu: (e: MouseEvent) => open(e),
    onKeyDown: (e: KeyboardEvent) => {
      const element = e.target as HTMLElement;
      if (element.closest("input, textarea, select") || element.isContentEditable) return;
      if (isContextKey(e)) open(e);
    },
  };
}
