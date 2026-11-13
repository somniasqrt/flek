import { createSignal } from "solid-js";
import { api } from "../api";
import { t } from "../i18n/index";
import { canView } from "../models/document";
import { openViewer, type Sender } from "./Viewer";

type Action = { busy: boolean; error: string };
const idle: Action = { busy: false, error: "" };
const [actions, setActions] = createSignal<Record<string, Action>>({});
export const attachmentAction = (id: string) => actions()[id] ?? idle;

// Keep state by id: the DB change can replace a thumbnail card while its fetch finishes.
export async function openAttachment(id: string, from?: Sender): Promise<boolean> {
  if (attachmentAction(id).busy) return false;
  const update = (action: Action) => setActions((previous) => ({ ...previous, [id]: action }));
  update({ busy: true, error: "" });
  try {
    // Cache hits are reconciled too; the return value is a fresh SQLite read.
    const file = await api.fetchFile(id);
    if (canView(file)) openViewer(file, from);
    else await api.openFile(id);
    update(idle);
    return true;
  } catch {
    update({ busy: false, error: t("Can't download or open this file. It may be unavailable, too large or unsupported.") });
    return false;
  }
}
