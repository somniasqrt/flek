import { Show } from "solid-js";
import { t } from "../i18n/index";
import { pick } from "../platform";
import { canOpen, type FilePresentation } from "../models/file-presentation";

export const revealLabel = () => pick({
  mac: t("Show in Finder"),
  windows: t("Show in File Explorer"),
  linux: t("Show in file manager"),
});
export const fileActionLabel = (file: FilePresentation) => canOpen(file) ? t("Open in default app") : revealLabel();
export default function FileSafety(props: { file: FilePresentation }) {
  return <Show when={!canOpen(props.file)}><span class="muted file-safety">{t("This file type can run code. Flek doesn't open it directly.")} {revealLabel()}</span></Show>;
}
