import { Match, Show, Switch } from "solid-js";
import { t, errorMessage } from "./i18n";
import T from "./i18n/T";
import { api, type Status } from "./api";
import { isMac } from "./platform";

/// First launch and lost sessions: import from Slack desktop, else sign in.
export default function Onboarding(props: { status: Status }) {
  return (
    <main class="onboarding" data-tauri-drag-region>
      <div class="card">
        <img class="logo" src="/icon.png" alt="" />
        <h1>Flek</h1>
        <Switch>
          <Match when={props.status.phase === "starting"}>
            <p class="muted">{t("Starting…")}</p>
          </Match>
          <Match when={props.status.phase === "importing"}>
            <p>{t("Taking your session from the Slack desktop app.")}</p>
            <Show when={isMac()}>
              <p class="muted">
                <T
                  k="macOS will ask to allow access to “Slack Safe Storage”. Choose <b>Always Allow</b>: Flek only reads the session cookie with it."
                  tags={{ b: (text) => <b>{text}</b> }}
                />
              </p>
            </Show>
            <div class="spinner" />
          </Match>
          <Match when={props.status.phase === "signing_in"}>
            <p>{t("Sign in to Slack in the window that just opened.")}</p>
            <p class="muted">
              {t(
                "Signing in with a code sent to your email works best. Flek takes over as soon as you are in.",
              )}
            </p>
            <div class="spinner" />
          </Match>
          <Match when={props.status.phase === "sign_in" && props.status}>
            {(s) => {
              const st = s() as Extract<Status, { phase: "sign_in" }>;
              return (
                <>
                  <p>{t("Flek reads Slack with your own session and keeps everything it sees.")}</p>
                  <Show when={st.reason}>
                    <p class="error">{errorMessage(st.reason)}</p>
                  </Show>
                  <button class="primary" onClick={() => api.signIn()}>
                    {t("Sign in to Slack")}
                  </button>
                  <Show when={st.desktop}>
                    <button class="link" onClick={() => api.importDesktop()}>
                      {t("Try the Slack desktop app again")}
                    </button>
                  </Show>
                </>
              );
            }}
          </Match>
        </Switch>
      </div>
    </main>
  );
}
