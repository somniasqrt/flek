import type { Conversation } from "../api";
import { sortDms } from "./settings-model.ts";

export function sidebarOrder(
  rows: readonly Conversation[],
  sort: "recent" | "name",
  locale: string,
  muted: boolean,
) {
  const live = rows.filter((c) => !c.is_archived);
  const normal = live.filter((c) => !c.starred && c.notification !== "muted");
  return [
    ...live.filter((c) => c.starred && c.notification !== "muted"),
    ...normal
      .filter((c) => c.kind.endsWith("channel"))
      .sort((a, b) => a.label.localeCompare(b.label)),
    ...sortDms(
      normal.filter((c) => c.kind === "im" || c.kind === "mpim"),
      sort,
      locale,
    ),
    ...live.filter((c) => muted && c.notification === "muted"),
  ];
}
