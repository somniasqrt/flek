import { createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "./i18n";

export default function ChannelsMenu(props: {
  x: number;
  y: number;
  onClose: () => void;
  onBrowse: () => void;
  onCreate: () => void;
}) {
  let panel!: HTMLDivElement;
  const [position, setPosition] = createSignal({ left: props.x, top: props.y });
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const fit = () => setPosition({
      left: Math.max(8, Math.min(props.x, window.innerWidth - panel.offsetWidth - 8)),
      top: Math.max(8, Math.min(props.y, window.innerHeight - panel.offsetHeight - 8)),
    });
    const observer = new ResizeObserver(fit);
    observer.observe(panel);
    fit();
    panel.querySelector<HTMLButtonElement>("button")?.focus();
    const outside = (e: PointerEvent) => {
      if (!panel.contains(e.target as Node)) props.onClose();
    };
    window.addEventListener("pointerdown", outside);
    window.addEventListener("blur", props.onClose);
    window.addEventListener("resize", props.onClose);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("blur", props.onClose);
      window.removeEventListener("resize", props.onClose);
      previous?.focus({ preventScroll: true });
    });
  });
  const run = (action: () => void) => {
    props.onClose();
    action();
  };
  const keydown = (e: KeyboardEvent) => {
    if (["Escape", "Tab"].includes(e.key)) {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      e.preventDefault();
      const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button")];
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      let next = index + (e.key === "ArrowUp" ? -1 : 1);
      if (e.key === "Home") next = 0;
      if (e.key === "End") next = buttons.length - 1;
      buttons[(next + buttons.length) % buttons.length]?.focus();
    }
  };
  return (
    <Portal>
      <div
        class="message-menu conversation-menu"
        ref={panel}
        role="menu"
        aria-label={t("Channels")}
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        onKeyDown={keydown}
      >
        <p class="muted menu-note">{t("Channels")}</p>
        <button role="menuitem" onClick={() => run(props.onCreate)}>{t("Create a channel")}</button>
        <button role="menuitem" onClick={() => run(props.onBrowse)}>{t("Browse channels")}</button>
      </div>
    </Portal>
  );
}
