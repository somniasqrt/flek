// Session-free preview. All content and IPC live under preview/.
import { render } from "solid-js/web";
import App from "./App";
import { initPlatform, setPlatform, type Platform } from "./platform";
import { setPreferencesOpen } from "./stores/settings";
import { query } from "./preview/mock-ipc";
import { installCache } from "./preview/assets";
import "./styles.css";

installCache();
// ?platform=windows|linux|mac previews the per-OS chrome, fonts and wording.
const forced = query.get("platform") as Platform | null;
await initPlatform(async () => forced ?? "");
if (forced) setPlatform(forced);
render(() => <App />, document.getElementById("root")!);
if (query.has("preferences")) setPreferencesOpen(true);
