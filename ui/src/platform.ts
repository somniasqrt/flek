// The single place that knows which OS the UI runs on. Shortcuts, wording,
// window chrome (CSS keyed on `html[data-platform]`) and fonts all derive
// from it. The Rust side is `crates/app/src/platform.rs`.
import { invoke } from "@tauri-apps/api/core";

export type Platform = "mac" | "windows" | "linux";

/// Maps Rust's `std::env::consts::OS` or a browser's platform/user agent string.
export function parsePlatform(value: string): Platform | undefined {
  if (/mac|iphone|ipad/i.test(value)) return "mac";
  if (/win/i.test(value) && !/darwin/i.test(value)) return "windows";
  if (/linux|x11|bsd|cros/i.test(value)) return "linux";
}

export function detectPlatform(nav?: { platform?: string; userAgent?: string }): Platform {
  return parsePlatform(nav?.platform ?? "") ?? parsePlatform(nav?.userAgent ?? "") ?? "linux";
}

let current: Platform = detectPlatform(typeof navigator === "undefined" ? undefined : navigator);

export const platform = (): Platform => current;
export const isMac = () => current === "mac";

/// Overrides the detected platform (startup, tests, the browser preview).
export function setPlatform(value: Platform) {
  current = value;
  if (typeof document !== "undefined") document.documentElement.dataset.platform = value;
}

/// Asks the app shell for the real OS; keeps the navigator guess when it is
/// unavailable (browser preview). Call once before the first render.
export async function initPlatform(
  ask: () => Promise<string> = () => invoke<string>("platform"),
) {
  let detected: Platform | undefined;
  try {
    detected = parsePlatform(await ask());
  } catch {
    // Not running inside Tauri.
  }
  setPlatform(detected ?? current);
}

/// Picks a per-platform value, e.g. a translated whole sentence.
export function pick<T>(options: Record<Platform, T>, p: Platform = current): T {
  return options[p];
}

export const modKeyLabel = (p: Platform = current) => (p === "mac" ? "⌘" : "Ctrl");
export const shiftKeyLabel = (p: Platform = current) => (p === "mac" ? "⇧" : "Shift");
/// Joins keys the way the platform writes them: `⌘⇧K` or `Ctrl+Shift+K`.
export const keyCombo = (keys: string[], p: Platform = current) =>
  p === "mac" ? keys.join("") : keys.join("+");

export const fileManagerName = (p: Platform = current) =>
  pick({ mac: "Finder", windows: "File Explorer", linux: "file manager" }, p);
export const credentialStoreName = (p: Platform = current) =>
  pick({ mac: "Keychain", windows: "Credential Manager", linux: "the system keyring" }, p);
export const trayName = (p: Platform = current) =>
  pick({ mac: "menu bar", windows: "system tray", linux: "system tray" }, p);
