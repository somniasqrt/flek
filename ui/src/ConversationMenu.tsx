import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api, type Conversation } from "./api";
import { conversationActions, hasUnread } from "./conversation-model";

export type ConversationMenuTarget = { id: string; x: number; y: number };

export function menuTarget(e: MouseEvent | KeyboardEvent, id: string): ConversationMenuTarget {
  e.preventDefault();
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  return {
    id,
    x: e instanceof MouseEvent && e.button === 2 ? e.clientX : rect.left + 24,
    y: e instanceof MouseEvent && e.button === 2 ? e.clientY : rect.bottom,
  };
}

export function isContextKey(e: KeyboardEvent) {
  return e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey);
}

export default function ConversationMenu(props: {
  conversation: Conversation;
  x: number;
  y: number;
  onClose: () => void;
  onDetails: () => void;
  onLeave: (conversation: Conversation) => void;
  onError: (message: string) => void;
}) {
  const c = () => props.conversation;
  const actions = () => conversationActions(c().kind);
  const [submenu, setSubmenu] = createSignal<"copy" | "notify" | null>(null);
  const [position, setPosition] = createSignal({ left: props.x, top: props.y });
  let panel!: HTMLDivElement;
  const [subPosition, setSubPosition] = createSignal({ left: 0, top: 0 });
  createEffect(() => {
    const name = submenu();
    position();
    if (!name) return;
    queueMicrotask(() => {
      const child = panel.querySelector<HTMLElement>(".conversation-submenu");
      const anchor = panel.querySelector<HTMLElement>(`[data-submenu="${name}"]`);
      if (!child || !anchor) return;
      const rect = panel.getBoundingClientRect();
      const right = rect.right - 4;
      const left = right + child.offsetWidth < window.innerWidth - 8
        ? right : rect.left - child.offsetWidth + 4;
      setSubPosition({
        left: Math.max(8, left),
        top: Math.max(8, Math.min(anchor.getBoundingClientRect().top,
          window.innerHeight - child.offsetHeight - 8)),
      });
    });
  });
  const run = async (action: () => Promise<unknown>) => {
    try {
      // Capture action arguments while the menu's conversation accessor is still live.
      const task = action();
      props.onClose();
      await task;
    } catch (e) {
      props.onClose();
      props.onError(String(e));
    }
  };
  const copy = (value: string) => run(() => navigator.clipboard.writeText(value));
  const notification = (value: string) => run(() =>
    api.conversationPreference(c().id, null, value),
  );
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const fit = () => setPosition({
      left: Math.max(8, Math.min(props.x, window.innerWidth - panel.offsetWidth - 8)),
      top: Math.max(8, Math.min(props.y, window.innerHeight - panel.offsetHeight - 8)),
    });
    const observer = new ResizeObserver(fit);
    observer.observe(panel);
    fit();
    panel.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const outside = (e: PointerEvent) => {
      if (!panel.contains(e.target as Node)) props.onClose();
    };
    const dismiss = () => props.onClose();
    window.addEventListener("pointerdown", outside);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);
      previous?.focus({ preventScroll: true });
    });
  });
  const keydown = (e: KeyboardEvent) => {
    const target = e.target as HTMLElement;
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    }
    if (e.key === "ArrowRight" && target.dataset.submenu) {
      e.preventDefault();
      setSubmenu(target.dataset.submenu as "copy" | "notify");
      queueMicrotask(() => panel.querySelector<HTMLElement>(".conversation-submenu button")?.focus());
    }
    if (e.key === "ArrowLeft" && submenu()) {
      e.preventDefault();
      const name = submenu();
      setSubmenu(null);
      panel.querySelector<HTMLElement>(`[data-submenu="${name}"]`)?.focus();
    }
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
      e.preventDefault();
      const menu = target.closest('[role="menu"]') ?? panel;
      const buttons = [...menu.querySelectorAll<HTMLButtonElement>(":scope > button:not(:disabled)")];
      const index = buttons.indexOf(target as HTMLButtonElement);
      let next = e.key === "ArrowUp" ? index - 1 : index + 1;
      if (e.key === "Home") next = 0;
      if (e.key === "End") next = buttons.length - 1;
      buttons[(next + buttons.length) % buttons.length]?.focus();
    }
  };
  const closeConversation = () => {
    if (actions().channel) {
      props.onLeave(c());
      props.onClose();
    } else {
      void run(() => api.dismissConversation(c().id));
    }
  };
  return (
    <Portal>
      <div
        class="message-menu conversation-menu"
        ref={panel}
        role="menu"
        aria-label={`Actions for ${c().label}`}
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        onKeyDown={keydown}
        onContextMenu={(e) => e.preventDefault()}
      >
        <button
          role="menuitem"
          disabled={!hasUnread(c()) || !c().latest_ts}
          onMouseEnter={() => setSubmenu(null)}
          onClick={() => run(() => api.markRead(c().id, c().latest_ts!))}
        >Mark as read</button>
        <button
          role="menuitem"
          onMouseEnter={() => setSubmenu(null)}
          onClick={() => { props.onDetails(); props.onClose(); }}
        >{actions().details}{actions().channel ? " ›" : ""}</button>
        <div class="menu-separator" role="separator" />
        <button
          role="menuitem"
          data-submenu="copy"
          aria-haspopup="menu"
          aria-expanded={submenu() === "copy"}
          onMouseEnter={() => setSubmenu("copy")}
          onClick={() => setSubmenu("copy")}
        >Copy <span>›</span></button>
        <button
          role="menuitem"
          onMouseEnter={() => setSubmenu(null)}
          onClick={() => run(() => api.conversationPreference(c().id, !c().starred, null))}
        >{c().starred ? "Unstar" : "Star channel"}</button>
        <button
          role="menuitem"
          data-submenu="notify"
          aria-haspopup="menu"
          aria-expanded={submenu() === "notify"}
          onMouseEnter={() => setSubmenu("notify")}
          onClick={() => setSubmenu("notify")}
        >Notify you about… <span>›</span></button>
        <div class="menu-separator" role="separator" />
        <button
          role="menuitem"
          class="danger"
          disabled={c().is_general}
          onMouseEnter={() => setSubmenu(null)}
          onClick={closeConversation}
        >{actions().dismiss}</button>
        <Show when={submenu()}>
          <div
            class="conversation-submenu"
            role="menu"
            aria-label={submenu()!}
            style={{ left: `${subPosition().left}px`, top: `${subPosition().top}px` }}
          >
            <Show when={submenu() === "copy"}>
              <button role="menuitem" onClick={() => copy(c().label)}>Copy name</button>
              <button role="menuitem" onClick={() => run(async () => {
                const link = await api.conversationLink(c().id);
                if (!link) throw new Error("Workspace link is not cached yet");
                await navigator.clipboard.writeText(link);
              })}>Copy link</button>
              <button role="menuitem" onClick={() => copy(c().id)}>Copy ID</button>
            </Show>
            <Show when={submenu() === "notify"}>
              <p class="muted menu-note">In Flek only</p>
              <For each={[
                ["all", "All new posts"], ["mentions", "Just mentions"],
                ["muted", "Mute and hide"],
              ]}>
                {([value, label]) => (
                  <button
                    role="menuitemradio"
                    aria-checked={(c().notification ?? "all") === value}
                    onClick={() => notification(value)}
                  >{(c().notification ?? "all") === value ? "✓ " : ""}{label}</button>
                )}
              </For>
            </Show>
          </div>
        </Show>
      </div>
    </Portal>
  );
}


export function LeaveConfirmation(props: {
  conversation: Conversation;
  onClose: () => void;
}) {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  let panel!: HTMLDivElement;
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.querySelector<HTMLButtonElement>("button")?.focus();
    onCleanup(() => previous?.focus({ preventScroll: true }));
  });
  const leave = async () => {
    setBusy(true);
    try {
      await api.dismissConversation(props.conversation.id);
      props.onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  const keydown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      if (!busy()) props.onClose();
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = current + (e.shiftKey ? -1 : 1);
      buttons[(next + buttons.length) % buttons.length]?.focus();
    }
  };
  return (
    <Portal>
      <div class="conversation-confirm-backdrop">
        <div
          class="conversation-confirm"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="leave-conversation-title"
          ref={panel}
          onKeyDown={keydown}
        >
          <h2 id="leave-conversation-title">Leave #{props.conversation.label}?</h2>
          <p>You will leave this channel in Slack. Flek keeps its cached history.</p>
          <Show when={error()}>
            <p class="error" role="alert">{error()}</p>
          </Show>
          <footer>
            <button disabled={busy()} onClick={props.onClose}>Cancel</button>
            <button class="danger" disabled={busy()} onClick={leave}>
              {busy() ? "Leaving…" : "Leave channel"}
            </button>
          </footer>
        </div>
      </div>
    </Portal>
  );
}
