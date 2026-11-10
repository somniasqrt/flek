import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { api, type ChannelAction } from "../api";
import { errorMessage, t } from "../i18n/index";
import { channelNameIssue, normalizeChannelName } from "../models/channel-browser-model";

export default function CreateChannel(props: {
  onCreated: (result: ChannelAction) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = createSignal("");
  const [privateChannel, setPrivateChannel] = createSignal(false);
  const [purpose, setPurpose] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  let panel!: HTMLFormElement;
  const issue = () => {
    switch (channelNameIssue(name())) {
      case "required": return t("Enter a channel name");
      case "length": return t("Channel names can have at most 80 characters");
      case "characters": return t("Use letters, numbers, hyphens and underscores");
      default: return "";
    }
  };
  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.querySelector<HTMLInputElement>("input")?.focus();
    onCleanup(() => previous?.focus({ preventScroll: true }));
  });
  const submit = async (e: SubmitEvent) => {
    e.preventDefault();
    if (busy() || issue()) return;
    setBusy(true);
    setError("");
    try {
      const normalized = await api.validateChannelName(name());
      const result = await api.createChannel(normalized, privateChannel(), purpose());
      // Creation has succeeded: dismiss before navigation, never offer a duplicate retry.
      props.onClose();
      await props.onCreated(result);
    } catch (e) {
      setError(String(e));
    } finally {
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
      const controls = [...panel.querySelectorAll<HTMLElement>(
        "input:not(:disabled), select:not(:disabled), textarea:not(:disabled), button:not(:disabled)",
      )];
      const index = controls.indexOf(document.activeElement as HTMLElement);
      if ((e.shiftKey && index <= 0) || (!e.shiftKey && index === controls.length - 1)) {
        e.preventDefault();
        controls[e.shiftKey ? controls.length - 1 : 0]?.focus();
      }
    }
  };
  return (
    <Portal>
      <div class="conversation-confirm-backdrop">
        <form
          class="conversation-confirm create-channel"
          ref={panel}
          role="dialog"
          aria-modal="true"
          aria-labelledby="create-channel-title"
          onSubmit={submit}
          onKeyDown={keydown}
        >
          <h2 id="create-channel-title">{t("Create a channel")}</h2>
          <label>
            {t("Channel name")}
            <input
              value={name()}
              onInput={(e) => setName(normalizeChannelName(e.currentTarget.value))}
              aria-describedby="channel-name-validation"
              aria-invalid={!!issue() && !!name()}
              disabled={busy()}
            />
          </label>
          <p class="muted" id="channel-name-validation" aria-live="polite">
            {issue() || t("Use letters, numbers, hyphens and underscores")}
          </p>
          <label>
            {t("Visibility")}
            <select
              value={privateChannel() ? "private" : "public"}
              onChange={(e) => setPrivateChannel(e.currentTarget.value === "private")}
              disabled={busy()}
            >
              <option value="public">{t("Public")}</option>
              <option value="private">{t("Private")}</option>
            </select>
          </label>
          <p class="muted">
            {privateChannel()
              ? t("Only invited people can find and join a private channel.")
              : t("Anyone in this workspace can find and join a public channel.")}
          </p>
          <label>
            {t("Description (optional)")}
            <textarea
              value={purpose()}
              maxLength={250}
              onInput={(e) => setPurpose(e.currentTarget.value)}
              disabled={busy()}
            />
          </label>
          <Show when={error()}>
            <p class="error" role="alert">{errorMessage(error())}</p>
          </Show>
          <footer>
            <button type="button" disabled={busy()} onClick={props.onClose}>{t("Cancel")}</button>
            <button type="submit" disabled={busy() || !!issue()}>
              {busy() ? t("Creating…") : t("Create channel")}
            </button>
          </footer>
        </form>
      </div>
    </Portal>
  );
}
