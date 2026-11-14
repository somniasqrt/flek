import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "../i18n/index";
import { api } from "../api";
import { isContextKey } from "./ConversationMenu";

export type ObjectAction = { label: string; run: () => unknown; disabled?: boolean };
type Target = { x: number; y: number; label: string; items: ObjectAction[] };
const [target, setTarget] = createSignal<Target | null>(null);
const [error, setError] = createSignal("");

// Native editing and Copy remain available exactly where the pointer hits text.
function nativeContext(e: MouseEvent) {
  const element = e.target instanceof Element ? e.target : null;
  if (element?.closest("input, textarea, select") || (element instanceof HTMLElement && element.isContentEditable)) return true;
  const selection = window.getSelection();
  if (!selection?.toString()) return false;
  for (let i = 0; i < selection.rangeCount; i++) {
    for (const rect of selection.getRangeAt(i).getClientRects()) {
      if (e.clientX >= rect.left && e.clientX <= rect.right && e.clientY >= rect.top && e.clientY <= rect.bottom) return true;
    }
  }
  return false;
}

export function openObjectMenu(e: MouseEvent | KeyboardEvent, label: string, items: ObjectAction[]) {
  e.preventDefault();
  e.stopPropagation();
  const element = e.currentTarget as HTMLElement;
  const rect = element.getBoundingClientRect();
  setError("");
  setTarget({
    x: e instanceof MouseEvent ? e.clientX : rect.left + 16,
    y: e instanceof MouseEvent ? e.clientY : rect.bottom,
    label, items,
  });
}

function linkMenu(e: MouseEvent | KeyboardEvent, url: string, element: HTMLElement) {
  e.preventDefault();
  e.stopPropagation();
  const rect = element.getBoundingClientRect();
  setError("");
  setTarget({
    x: e instanceof MouseEvent ? e.clientX : rect.left,
    y: e instanceof MouseEvent ? e.clientY : rect.bottom,
    label: t("Link actions"),
    items: [
      { label: /^https?:/i.test(url) ? t("Open in browser") : t("Open link"), run: () => api.openUrl(url) },
      { label: t("Copy link"), run: () => navigator.clipboard.writeText(url) },
    ],
  });
}

export default function ObjectMenus() {
  onMount(() => {
    // One main-document policy, shared by production and synthetic preview.
    const context = (e: MouseEvent) => {
      if (nativeContext(e)) { e.stopPropagation(); return; }
      e.preventDefault();
      const element = e.target instanceof Element ? e.target : null;
      const link = element?.closest<HTMLElement>("[data-external-url]");
      if (link?.dataset.externalUrl) linkMenu(e, link.dataset.externalUrl, link);
    };
    const key = (e: KeyboardEvent) => {
      if (!isContextKey(e)) return;
      const element = e.target instanceof Element ? e.target : null;
      if (element?.closest("input, textarea, select") || (element instanceof HTMLElement && element.isContentEditable)) return;
      const link = element?.closest<HTMLElement>("[data-external-url]");
      if (link?.dataset.externalUrl) linkMenu(e, link.dataset.externalUrl, link);
    };
    document.addEventListener("contextmenu", context, true);
    document.addEventListener("keydown", key, true);
    onCleanup(() => {
      document.removeEventListener("contextmenu", context, true);
      document.removeEventListener("keydown", key, true);
    });
  });
  return <>
    <Show when={target()} keyed>{(menu) => <Menu menu={menu} />}</Show>
    <Show when={error()}><div class="file-notice error" role="alert">{error()} <button onClick={() => setError("")}>{t("Close")}</button></div></Show>
  </>;
}

function Menu(props: { menu: Target }) {
  let panel!: HTMLDivElement;
  const [position, setPosition] = createSignal({ left: props.menu.x, top: props.menu.y });
  const close = () => setTarget(null);
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const fit = () => setPosition({
      left: Math.max(8, Math.min(props.menu.x, window.innerWidth - panel.offsetWidth - 8)),
      top: Math.max(8, Math.min(props.menu.y, window.innerHeight - panel.offsetHeight - 8)),
    });
    const observer = new ResizeObserver(fit);
    observer.observe(panel);
    fit();
    panel.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
      previous?.focus({ preventScroll: true });
    });
  });
  const keydown = (e: KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Tab") { e.preventDefault(); close(); }
    if (["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
      e.preventDefault();
      const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === "Home" ? 0 : e.key === "End" ? buttons.length - 1
        : (index + (["ArrowUp", "ArrowLeft"].includes(e.key) ? -1 : 1) + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  };
  return <Portal>
    <div class="message-menu-backdrop" onPointerDown={close} onContextMenu={(e) => { e.preventDefault(); close(); }}>
      <div ref={panel} class="message-menu" role="menu" aria-label={props.menu.label}
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); }} onKeyDown={keydown}>
        <For each={props.menu.items}>{(item) => <button class="message-menu-item" role="menuitem" disabled={item.disabled}
          onClick={() => {
            try {
              const task = item.run();
              close();
              Promise.resolve(task).catch(() => setError(t("Could not complete the file or link action.")));
            } catch { close(); setError(t("Could not complete the file or link action.")); }
          }}><span class="message-menu-label">{item.label}</span></button>}</For>
      </div>
    </div>
  </Portal>;
}
