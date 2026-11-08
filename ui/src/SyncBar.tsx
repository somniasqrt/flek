import { Show } from "solid-js";
import type { ConnectionState, Progress } from "./api";

/// Thin bar above the timeline while the engine works or the network is gone.
export default function SyncBar(props: {
  progress: Progress | null;
  connection: ConnectionState | null;
}) {
  const text = () => {
    if (props.connection?.state === "reconnecting") return "Offline, reconnecting…";
    const p = props.progress;
    if (!p || p.stage === "idle") return null;
    switch (p.stage) {
      case "directory":
        return "Syncing channels and people…";
      case "conversations":
        return p.deep
          ? `Fetching older history · ${p.current}`
          : `Syncing messages · ${p.current}`;
      case "files":
        return "Downloading files…";
    }
  };
  const fraction = () => {
    const p = props.progress;
    if (!p || (p.stage !== "conversations" && p.stage !== "files") || p.total === 0) return null;
    return Math.min(1, p.done / p.total);
  };

  return (
    <Show when={text()}>
      {(t) => (
        <div class="syncbar" classList={{ offline: props.connection?.state === "reconnecting" }}>
          <span>{t()}</span>
          <Show when={fraction() !== null}>
            <div class="bar">
              <div class="fill" style={{ width: `${(fraction() ?? 0) * 100}%` }} />
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
}
