import { t } from "./i18n";
import type { ConnectionState, Progress } from "./api";

/// Status line above the timeline. Always the same height, so the timeline
/// never jumps when syncing starts or stops.
export default function SyncBar(props: {
  progress: Progress | null;
  connection: ConnectionState | null;
}) {
  const text = () => {
    if (props.connection?.state === "reconnecting") return t("Offline, reconnecting…");
    const p = props.progress;
    if (!p || p.stage === "idle") return t("Up to date");
    switch (p.stage) {
      case "directory":
        return t("Syncing channels and people…");
      case "conversations":
        return p.deep
          ? t("Fetching older history · {name}", { name: p.current })
          : t("Syncing messages · {name}", { name: p.current });
      case "files":
        return t("Downloading files…");
    }
  };
  const fraction = () => {
    const p = props.progress;
    if (!p || (p.stage !== "conversations" && p.stage !== "files") || p.total === 0) return null;
    return Math.min(1, p.done / p.total);
  };

  return (
    <div class="syncbar" classList={{ offline: props.connection?.state === "reconnecting" }}>
      <span>{text()}</span>
      <div class="bar" classList={{ hidden: fraction() === null }}>
        <div class="fill" style={{ width: `${(fraction() ?? 0) * 100}%` }} />
      </div>
    </div>
  );
}
