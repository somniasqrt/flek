import ObjectMenus from "./components/ObjectMenu";
import DownloadNotice from "./components/Download";
import { createSignal, Match, onCleanup, onMount, Switch } from "solid-js";
import { Show } from "solid-js";
import { api, onStatus, type Status } from "./api";
import Onboarding from "./views/Onboarding";
import Workspace from "./views/Workspace";
import Preferences from "./views/Preferences";
import { observeSettings, preferencesOpen, setPreferencesOpen } from "./stores/settings";
import { listen } from "@tauri-apps/api/event";
import { primaryModifier, usesMacShortcuts } from "./models/shortcuts";

export default function App() {
  const [status, setStatus] = createSignal<Status>({ phase: "starting" });

  onMount(async () => {
    // Listen first, then read: a status sent in between is not lost.
    const preferences = listen("flek://preferences", () => setPreferencesOpen(true));
    const settings = observeSettings();
    const key = (e: KeyboardEvent) => {
      if (
        e.code === "Comma" &&
        primaryModifier(e, usesMacShortcuts()) &&
        !e.shiftKey &&
        !e.altKey &&
        !e.isComposing
      ) {
        e.preventDefault();
        setPreferencesOpen(true);
      }
    };
    window.addEventListener("keydown", key);
    onCleanup(() => {
      preferences.then((f) => f());
      settings.then((f) => f()).catch(() => {});
      window.removeEventListener("keydown", key);
    });
    const unlisten = onStatus(setStatus);
    onCleanup(() => unlisten.then((f) => f()));
    await unlisten;
    setStatus(await api.status());
  });

  return (
    <>
      <ObjectMenus />
      <DownloadNotice />
      <Show when={preferencesOpen()}>
        <Preferences status={status()} />
      </Show>
      <Switch fallback={<Onboarding status={status()} />}>
        <Match when={status().phase === "ready" && status()}>
          {(s) => <Workspace status={s() as Extract<Status, { phase: "ready" }>} />}
        </Match>
      </Switch>
    </>
  );
}
