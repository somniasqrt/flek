import { Match, Show, Switch } from "solid-js";
import { api, type Status } from "./api";

/// First launch and lost sessions: import from Slack desktop, else sign in.
export default function Onboarding(props: { status: Status }) {
  return (
    <main class="onboarding" data-tauri-drag-region>
      <div class="card">
        <img class="logo" src="/icon.png" alt="" />
        <h1>Flek</h1>
        <Switch>
          <Match when={props.status.phase === "starting"}>
            <p class="muted">Starting…</p>
          </Match>
          <Match when={props.status.phase === "importing"}>
            <p>Taking your session from the Slack desktop app.</p>
            <p class="muted">
              macOS will ask to allow access to “Slack Safe Storage”. Choose
              <b> Always Allow</b>: Flek only reads the session cookie with it.
            </p>
            <div class="spinner" />
          </Match>
          <Match when={props.status.phase === "signing_in"}>
            <p>Sign in to Slack in the window that just opened.</p>
            <p class="muted">
              Signing in with a code sent to your email works best. Flek takes over as soon as you
              are in.
            </p>
            <div class="spinner" />
          </Match>
          <Match when={props.status.phase === "sign_in" && props.status}>
            {(s) => {
              const st = s() as Extract<Status, { phase: "sign_in" }>;
              return (
                <>
                  <p>Flek reads Slack with your own session and keeps everything it sees.</p>
                  <Show when={st.reason}>
                    <p class="error">{st.reason}</p>
                  </Show>
                  <button class="primary" onClick={() => api.signIn()}>
                    Sign in to Slack
                  </button>
                  <Show when={st.desktop}>
                    <button class="link" onClick={() => api.importDesktop()}>
                      Try the Slack desktop app again
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
