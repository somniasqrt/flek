// Message box at the bottom of a conversation or thread (like Slack's):
// Enter sends, Shift+Enter breaks the line, drafts survive restarts.
import { createEffect, createSignal, on, onCleanup, onMount, Show } from "solid-js";
import { draftKey, outgoingText, readDraft, writeDraft } from "./compose";
import { send } from "./outbox";

export default function Composer(props: {
  conversationId: string;
  /// Reply in this thread.
  threadTs?: string;
  placeholder: string;
  /// Thread composer: label of the "Also send to #channel" checkbox.
  broadcastLabel?: string;
  /// After a message was queued (the list scrolls to it).
  onSent?: () => void;
}) {
  const key = () => draftKey(props.conversationId, props.threadTs);
  const [text, setText] = createSignal("");
  const [broadcast, setBroadcast] = createSignal(false);
  let area!: HTMLTextAreaElement;

  // Grows with the text; CSS caps it (max-height) and scrolls beyond.
  const resize = () => {
    area.style.height = "auto";
    area.style.height = `${area.scrollHeight}px`;
  };

  // Another conversation or thread: its own draft, and the cursor in the box.
  createEffect(
    on(key, (k) => {
      setText(readDraft(k));
      setBroadcast(false);
      requestAnimationFrame(() => {
        resize();
        area.focus({ preventScroll: true });
      });
    }),
  );
  onMount(() => {
    window.addEventListener("resize", resize);
    onCleanup(() => window.removeEventListener("resize", resize));
  });

  const onInput = (value: string) => {
    setText(value);
    writeDraft(key(), value);
    resize();
  };

  const submit = () => {
    const t = outgoingText(text());
    if (!t) return;
    send(props.conversationId, t, props.threadTs ?? null, broadcast());
    setText("");
    writeDraft(key(), "");
    setBroadcast(false);
    resize();
    props.onSent?.();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    // isComposing: Enter that confirms an IME candidate does not send.
    if (e.key !== "Enter" || e.shiftKey || e.isComposing || e.keyCode === 229) return;
    e.preventDefault();
    submit();
  };

  return (
    <div class="composer">
      <div class="composer-box" onClick={(e) => e.target === e.currentTarget && area.focus()}>
        <textarea
          ref={area}
          rows={1}
          value={text()}
          placeholder={props.placeholder}
          aria-label={props.placeholder}
          spellcheck={true}
          onInput={(e) => onInput(e.currentTarget.value)}
          onKeyDown={onKeyDown}
        />
        <div class="composer-bar">
          <Show when={props.broadcastLabel}>
            <label class="composer-broadcast">
              <input
                type="checkbox"
                checked={broadcast()}
                onChange={(e) => {
                  setBroadcast(e.currentTarget.checked);
                  // Back to the text, so Enter still sends.
                  area.focus();
                }}
              />
              <span>{props.broadcastLabel}</span>
            </label>
          </Show>
          <span class="composer-hint muted" classList={{ shown: !!text() }}>
            <b>Shift + Enter</b> for a new line
          </span>
          <button
            class="composer-send"
            type="button"
            title="Send (Enter)"
            aria-label="Send"
            disabled={!outgoingText(text())}
            onClick={submit}
          >
            <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
              <path d="M2.6 2.9a.75.75 0 0 1 .82-.12l14 6.5a.75.75 0 0 1 0 1.36l-14 6.5a.75.75 0 0 1-1.04-.86L4.1 10 2.38 3.72a.75.75 0 0 1 .22-.82zM5.4 10.75l-1.2 4.4L15.3 10 4.2 4.85l1.2 4.4H10a.75.75 0 0 1 0 1.5z" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  );
}
