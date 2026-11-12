import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api } from "../api";
import { t, errorMessage } from "../i18n/index";
import { localDateBounds } from "../models/virtualizer";

export default function JumpToDate(props: {
  conversationId: string;
  initial: string;
  onClose: () => void;
  onJump: (ts: string) => Promise<unknown>;
}) {
  const [date, setDate] = createSignal(props.initial);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  let panel!: HTMLDivElement;
  let input!: HTMLInputElement;
  let disposed = false;
  onCleanup(() => {
    disposed = true;
  });
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.focus();
    onCleanup(() => previous?.isConnected && previous.focus({ preventScroll: true }));
  });
  const submit = async () => {
    const bounds = localDateBounds(date());
    if (!bounds || busy()) return;
    setBusy(true);
    setError("");
    try {
      const result = await api.dateTarget(props.conversationId, bounds.start, bounds.end);
      if (disposed) return;
      if (result.status === "before_archive")
        setError(t("This date is older than the history stored in Flek."));
      else if (result.status === "empty") setError(t("No messages are stored for this date."));
      else {
        await props.onJump(result.ts);
        if (!disposed) props.onClose();
      }
    } catch (e) {
      if (!disposed) setError(errorMessage(e));
    } finally {
      if (!disposed) setBusy(false);
    }
  };
  return (
    <Portal>
      <div
        class="new-message-backdrop"
        onClick={(e) => e.target === e.currentTarget && props.onClose()}
      >
        <div
          class="new-message-picker date-picker"
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-label={t("Jump to date")}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.stopPropagation();
              props.onClose();
            }
            if (e.key === "Tab") {
              const controls = [
                ...panel.querySelectorAll<HTMLElement>("input,button:not(:disabled)"),
              ];
              const i = controls.indexOf(document.activeElement as HTMLElement);
              if (e.shiftKey ? i <= 0 : i === controls.length - 1) {
                e.preventDefault();
                (e.shiftKey ? controls.at(-1) : controls[0])?.focus();
              }
            }
          }}
        >
          <header>
            <h2>{t("Jump to date")}</h2>
            <button onClick={props.onClose} aria-label={t("Close")}>
              ×
            </button>
          </header>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <input
              ref={input}
              type="date"
              required
              aria-label={t("Date")}
              value={date()}
              onInput={(e) => setDate(e.currentTarget.value)}
            />
            <button type="submit" disabled={busy() || !localDateBounds(date())}>
              {t("Jump to date")}
            </button>
          </form>
          <Show when={error()}>
            <p class="error" role="alert">
              {error()}
            </p>
          </Show>
        </div>
      </div>
    </Portal>
  );
}
