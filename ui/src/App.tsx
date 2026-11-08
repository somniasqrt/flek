import { createSignal, Match, onCleanup, onMount, Switch } from "solid-js";
import { api, onStatus, type Status } from "./api";
import Onboarding from "./Onboarding";
import Workspace from "./Workspace";

export default function App() {
  const [status, setStatus] = createSignal<Status>({ phase: "starting" });

  onMount(async () => {
    // Listen first, then read: a status sent in between is not lost.
    const unlisten = onStatus(setStatus);
    onCleanup(() => unlisten.then((f) => f()));
    await unlisten;
    setStatus(await api.status());
  });

  return (
    <Switch fallback={<Onboarding status={status()} />}>
      <Match when={status().phase === "ready" && status()}>
        {(s) => <Workspace status={s() as Extract<Status, { phase: "ready" }>} />}
      </Match>
    </Switch>
  );
}
