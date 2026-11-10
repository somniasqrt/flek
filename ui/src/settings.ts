import { createSignal } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { setLanguage, setClock } from "./i18n";
import { defaultSettings, messageSize, type Settings } from "./settings-model";

export const [settings, replaceSettings] = createSignal<Settings>({ ...defaultSettings });
export const [preferencesOpen, setPreferencesOpen] = createSignal(false);

export function applySettings(value: Settings) {
  replaceSettings(value);
  setLanguage(value.language);
  setClock(value.clock_24);
  document.documentElement.dataset.theme = value.theme;
  document.documentElement.style.setProperty("--message-size", messageSize(value.text_size));
}

// Register before the first read so other native surfaces cannot race startup.
export async function observeSettings() {
  const unlisten = await listen<Settings>("flek://settings", (e) => applySettings(e.payload));
  try {
    applySettings(await api.getSettings());
  } catch (e) {
    unlisten();
    throw e;
  }
  return unlisten;
}

export async function changeSetting<K extends keyof Settings>(key: K, value: Settings[K]) {
  const next = { ...settings(), [key]: value };
  try {
    applySettings(await api.setSettings(next));
  } catch (error) {
    // Restore controlled inputs if OS integration or the atomic write failed.
    applySettings({ ...settings() });
    throw error;
  }
}
