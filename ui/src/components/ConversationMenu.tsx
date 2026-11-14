import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { t, errorMessage } from "../i18n/index";
import { openInSlack } from "../stores/slack";
import { api, type Conversation } from "../api";
import {
  conversationActions,
  conversationMenuPosition,
  conversationStarLabel,
  hasUnread,
} from "../models/conversation-model";

export type ConversationMenuTarget = { id: string; x: number; y: number };

export function menuTarget(
  e: MouseEvent | KeyboardEvent,
  id: string,
  placement: "sidebar" | "header" = "sidebar",
): ConversationMenuTarget {
  e.preventDefault();
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  return {
    id,
    x: placement === "header"
      ? rect.right
      : e instanceof MouseEvent && e.button === 2 ? e.clientX : rect.left + 24,
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
  placement?: "sidebar" | "header" | "notifications";
  onJumpDate?: () => void;
  onDetails: () => void;
  onLeave: (conversation: Conversation) => void;
  onError: (message: string) => void;
}) {
  const c = () => props.conversation;
  const header = () => props.placement === "header";
  const sidebar = () => !props.placement || props.placement === "sidebar";
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
      const left =
        right + child.offsetWidth < window.innerWidth - 8
          ? right
          : rect.left - child.offsetWidth + 4;
      setSubPosition({
        left: Math.max(8, left),
        top: Math.max(
          8,
          Math.min(anchor.getBoundingClientRect().top, window.innerHeight - child.offsetHeight - 8),
        ),
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
  const notification = (value: string) =>
    run(() => api.conversationPreference(c().id, null, value));
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const fit = () =>
      setPosition(conversationMenuPosition(
        props.x, props.y, panel.offsetWidth, panel.offsetHeight,
        window.innerWidth, window.innerHeight, !sidebar(),
      ));
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
      queueMicrotask(() =>
        panel.querySelector<HTMLElement>(".conversation-submenu button")?.focus(),
      );
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
      const buttons = [
        ...menu.querySelectorAll<HTMLButtonElement>(":scope > button:not(:disabled)"),
      ];
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
        classList={{ "conversation-header-menu": !sidebar() }}
        ref={panel}
        role="menu"
        aria-label={props.placement === "notifications" ? t("Notification preferences") : t("Actions for {name}", { name: c().label })}
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        onKeyDown={keydown}
        onContextMenu={(e) => e.preventDefault()}
      >
        <Show
          when={props.placement === "notifications"}
          fallback={<>
            <Show when={sidebar()}>
              <button
                role="menuitem"
                disabled={!hasUnread(c()) || !c().latest_ts}
                onMouseEnter={() => setSubmenu(null)}
                onClick={() => run(() => api.markRead(c().id, c().latest_ts!))}
              >
                {t("Mark as read")}
              </button>
            </Show>
            <button
              role="menuitem"
              onMouseEnter={() => setSubmenu(null)}
              onClick={() => {
                props.onDetails();
                props.onClose();
              }}
            >
              {actions().details}
              {sidebar() && actions().channel ? " ›" : ""}
            </button>
            <Show when={sidebar()}>
              <button role="menuitem" onMouseEnter={() => setSubmenu(null)} onClick={() => run(() => openInSlack(c().id))}>
                {t("Open in Slack")}
              </button>
              <div class="menu-separator" role="separator" />
            </Show>
            <button
              role="menuitem"
              data-submenu="copy"
              aria-haspopup="menu"
              aria-expanded={submenu() === "copy"}
              onMouseEnter={() => setSubmenu("copy")}
              onClick={() => setSubmenu("copy")}
            >
              {t("Copy")}
              <span>›</span>
            </button>
            <button
              role="menuitem"
              onMouseEnter={() => setSubmenu(null)}
              onClick={() => run(() => api.conversationPreference(c().id, !c().starred, null))}
            >
              {sidebar() ? c().starred ? t("Unstar") : t("Star channel") : conversationStarLabel(c())}
            </button>
            <Show when={sidebar()}>
              <button
                role="menuitem"
                data-submenu="notify"
                aria-haspopup="menu"
                aria-expanded={submenu() === "notify"}
                onMouseEnter={() => setSubmenu("notify")}
                onClick={() => setSubmenu("notify")}
              >
                {t("Notify you about…")}
                <span>›</span>
              </button>
            </Show>
            <Show when={header()}>
              <div class="menu-separator" role="separator" />
              <button
                role="menuitem"
                disabled={!hasUnread(c()) || !c().latest_ts}
                onMouseEnter={() => setSubmenu(null)}
                onClick={() => run(() => api.markRead(c().id, c().latest_ts!))}
              >
                {t("Mark as read")}
              </button>
              <button role="menuitem" onMouseEnter={() => setSubmenu(null)} onClick={() => {
                props.onClose();
                props.onJumpDate?.();
              }}>
                {t("Jump to date…")}
              </button>
              <div class="menu-separator" role="separator" />
              <button role="menuitem" onMouseEnter={() => setSubmenu(null)} onClick={() => run(() => openInSlack(c().id))}>
                {t("Open in Slack")}
              </button>
            </Show>
            <div class="menu-separator" role="separator" />
            <button
              role="menuitem"
              class="danger"
              disabled={c().is_general}
              onMouseEnter={() => setSubmenu(null)}
              onClick={closeConversation}
            >
              {actions().dismiss}
            </button>
            <Show when={submenu()}>
              <div
                class="conversation-submenu"
                role="menu"
                aria-label={submenu() === "copy" ? t("Copy") : t("Notify you about…")}
                style={{ left: `${subPosition().left}px`, top: `${subPosition().top}px` }}
              >
                <Show when={submenu() === "copy"}>
                  <button role="menuitem" onClick={() => copy(c().label)}>
                    {t("Copy name")}
                  </button>
                  <button
                    role="menuitem"
                    onClick={() =>
                      run(async () => {
                        const link = await api.conversationLink(c().id);
                        if (!link) throw new Error(t("Workspace link is not cached yet"));
                        await navigator.clipboard.writeText(link);
                      })
                    }
                  >
                    {t("Copy link")}
                  </button>
                  <button role="menuitem" onClick={() => copy(c().id)}>
                    {t("Copy ID")}
                  </button>
                </Show>
                <Show when={submenu() === "notify"}>
                  <NotificationChoices conversation={c()} onSelect={notification} />
                </Show>
              </div>
            </Show>
          </>}
        >
          <NotificationChoices conversation={c()} onSelect={notification} />
        </Show>
      </div>
    </Portal>
  );
}

function NotificationChoices(props: { conversation: Conversation; onSelect: (value: string) => void }) {
  return (
    <>
      <p class="muted menu-note">{t("In Flek only")}</p>
      <For each={[
        ["all", t("All new posts")],
        ["mentions", t("Just mentions")],
        ["muted", t("Mute and hide")],
      ]}>
        {([value, label]) => (
          <button
            role="menuitemradio"
            aria-checked={(props.conversation.notification ?? "all") === value}
            onClick={() => props.onSelect(value)}
          >
            {(props.conversation.notification ?? "all") === value ? "✓ " : ""}
            {label}
          </button>
        )}
      </For>
    </>
  );
}

export function LeaveConfirmation(props: { conversation: Conversation; onClose: () => void }) {
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
          <h2 id="leave-conversation-title">
            {t("Leave #{name}?", { name: props.conversation.label })}
          </h2>
          <p>{t("You will leave this channel in Slack. Flek keeps its cached history.")}</p>
          <Show when={error()}>
            <p class="error" role="alert">
              {errorMessage(error())}
            </p>
          </Show>
          <footer>
            <button disabled={busy()} onClick={props.onClose}>
              {t("Cancel")}
            </button>
            <button class="danger" disabled={busy()} onClick={leave}>
              {busy() ? t("Leaving…") : t("Leave channel")}
            </button>
          </footer>
        </div>
      </div>
    </Portal>
  );
}
