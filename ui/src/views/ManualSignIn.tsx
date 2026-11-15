import { createSignal, createUniqueId, onCleanup, Show } from "solid-js";
import { api } from "../api";
import { errorMessage, t } from "../i18n/index";

/** Keep credentials only in masked DOM fields until invoking the native command. */
export default function ManualSignIn(props: { busy: boolean; onBusyChange: (busy: boolean) => void }) {
  const id = createUniqueId();
  const [error, setError] = createSignal<string | null>(null);
  let token!: HTMLInputElement;
  let cookie!: HTMLInputElement;
  const clear = () => {
    if (token) token.value = "";
    if (cookie) cookie.value = "";
  };
  onCleanup(clear);

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (props.busy) return;
    props.onBusyChange(true);
    setError(null);
    try {
      const pending = api.manualSignIn(token.value, cookie.value);
      clear();
      await pending;
    } catch (e) {
      setError(errorMessage(e));
      queueMicrotask(() => { if (token?.isConnected) token.focus(); });
    } finally {
      clear();
      props.onBusyChange(false);
    }
  };

  return (
    <details class="manual-signin" onToggle={(e) => {
      if (!e.currentTarget.open) { clear(); setError(null); }
    }}>
      <summary>{t("Advanced")}</summary>
      <form onSubmit={submit} autocomplete="off" novalidate aria-busy={props.busy}>
        <p>{t("Use a browser session only if the other sign-in methods do not work. Treat both values as passwords.")}</p>
        <p class="muted" id={`${id}-help`}>
          {t("In browser DevTools, open Application (Storage in Firefox). Under Local Storage → https://app.slack.com → localConfig_v2, find teams.<team>.token. Under Cookies → https://app.slack.com, copy the d value.")}
        </p>
        <label for={`${id}-token`}>{t("Session token (xoxc-…)")}</label>
        <input ref={token} id={`${id}-token`} type="password" autocomplete="off"
          autocapitalize="off" spellcheck={false} maxlength={16384}
          aria-describedby={`${id}-help`} disabled={props.busy} />
        <label for={`${id}-cookie`}>{t("Session cookie (d)")}</label>
        <input ref={cookie} id={`${id}-cookie`} type="password" autocomplete="off"
          autocapitalize="off" spellcheck={false} maxlength={65536}
          aria-describedby={`${id}-cookie-help`} disabled={props.busy} />
        <p class="muted" id={`${id}-cookie-help`}>
          {t("Paste the xoxd-… value or d=…; URL-encoded values are accepted. Both fields are cleared after each attempt.")}
        </p>
        <Show when={error()}><p class="error" role="alert">{error()}</p></Show>
        <button class="primary" type="submit" disabled={props.busy}>
          {props.busy ? t("Validating session…") : t("Sign in with session")}
        </button>
      </form>
    </details>
  );
}
