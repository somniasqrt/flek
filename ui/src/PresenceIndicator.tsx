import { Show } from "solid-js";
import { t } from "./i18n";
import { Moon } from "./icons";
import type { Indicator } from "./presence";

const labels = (): Record<Indicator, string> => ({
  active: t("Active"),
  away: t("Away"),
  dnd: t("Notifications paused"),
  unknown: t("Presence unknown"),
});

export default function PresenceIndicator(props: { presence: Indicator }) {
  const label = () => labels()[props.presence];
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
