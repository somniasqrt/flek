import { createSignal, createUniqueId, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api, type Message } from "../api";
import { t, errorMessage } from "../i18n/index";
import Rich from "./Rich";

export default function DeleteMessage(props: {
  conversationId: string;
  message: Message;
  onClose: () => void;
  onUnstored: () => void;
}) {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const titleId = createUniqueId();
  let panel!: HTMLDivElement;
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.querySelector<HTMLButtonElement>("button")?.focus();
    onCleanup(() => { if (previous?.isConnected) previous.focus({ preventScroll: true }); });
  });
  const remove = async () => {
    if (busy()) return;
    setBusy(true);
    setError("");
    try {
      const changed = await api.deleteMessage(props.conversationId, props.message.ts);
      if (changed.not_stored_locally) props.onUnstored();
      props.onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  const keydown = (e: KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      if (!busy()) props.onClose();
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const buttons = [...panel.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      buttons[(current + (e.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    }
  };
  return (
    <Portal>
      <div class="conversation-confirm-backdrop" onPointerDown={(e) => { if (e.target === e.currentTarget && !busy()) props.onClose(); }}>
        <div ref={panel} class="conversation-confirm message-delete-confirm" role="alertdialog" aria-modal="true" aria-labelledby={titleId} aria-busy={busy()} onKeyDown={keydown}>
          <h2 id={titleId}>{t("Delete message?")}</h2>
          <p>{t("Are you sure you want to delete this message? This cannot be undone.")}</p>
          <div class="message-delete-preview"><Rich text={props.message.text} blocks={props.message.blocks} /></div>
          <Show when={error()}><p class="error" role="alert">{errorMessage(error())}</p></Show>
          <footer>
            <button disabled={busy()} onClick={props.onClose}>{t("Cancel")}</button>
            <button class="danger" disabled={busy()} onClick={remove}>{busy() ? t("Deleting…") : t("Delete message")}</button>
          </footer>
        </div>
      </div>
    </Portal>
  );
}
