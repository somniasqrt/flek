import { Show } from "solid-js";
import { Moon } from "./icons";
import type { Indicator } from "./presence";

const labels: Record<Indicator, string> = {
  active: "Active",
  away: "Away",
  dnd: "Notifications paused",
  unknown: "Presence unknown",
};

export default function PresenceIndicator(props: { presence: Indicator }) {
  const label = () => labels[props.presence];
  return (
    <span
      class={`presence-indicator presence-${props.presence}`}
      role="img"
      aria-label={label()}
      title={label()}
    >
      <Show when={props.presence === "dnd"}>
        <Moon size={12} />
      </Show>
    </span>
  );
}
