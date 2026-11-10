import { render } from "solid-js/web";
import App from "./App";
import { initPlatform } from "./platform";
import "./styles.css";

// The OS decides shortcuts, wording and chrome: know it before the first paint.
void initPlatform().then(() => render(() => <App />, document.getElementById("root")!));
