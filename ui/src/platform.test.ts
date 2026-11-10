import assert from "node:assert/strict";
import { test } from "node:test";
import {
  credentialStoreName,
  detectPlatform,
  fileManagerName,
  initPlatform,
  keyCombo,
  modKeyLabel,
  parsePlatform,
  pick,
  platform,
  setPlatform,
  trayName,
} from "./platform.ts";
import { shortcutLabel } from "./models/shortcuts.ts";

test("platform is parsed from Rust OS names, navigator.platform and user agents", () => {
  assert.equal(parsePlatform("macos"), "mac");
  assert.equal(parsePlatform("MacIntel"), "mac");
  assert.equal(parsePlatform("windows"), "windows");
  assert.equal(parsePlatform("Win32"), "windows");
  assert.equal(parsePlatform("linux"), "linux");
  assert.equal(parsePlatform("Linux x86_64"), "linux");
  assert.equal(parsePlatform("freebsd"), "linux");
  assert.equal(parsePlatform("plan9"), undefined);
  const windowsUa = { platform: "", userAgent: "Mozilla/5.0 (Windows NT 10.0)" };
  assert.equal(detectPlatform(windowsUa), "windows");
  assert.equal(detectPlatform({ platform: "MacIntel", userAgent: "" }), "mac");
  assert.equal(detectPlatform(undefined), "linux");
});

test("initPlatform trusts the app shell and survives its absence", async () => {
  await initPlatform(async () => "windows");
  assert.equal(platform(), "windows");
  await initPlatform(async () => {
    throw new Error("no tauri");
  });
  assert.equal(platform(), "windows");
  await initPlatform(async () => "macos");
  assert.equal(platform(), "mac");
});

test("wording and key labels per platform", () => {
  assert.deepEqual(
    (["mac", "windows", "linux"] as const).map((p) => [
      modKeyLabel(p),
      fileManagerName(p),
      credentialStoreName(p),
      trayName(p),
    ]),
    [
      ["⌘", "Finder", "Keychain", "menu bar"],
      ["Ctrl", "File Explorer", "Credential Manager", "system tray"],
      ["Ctrl", "file manager", "the system keyring", "system tray"],
    ],
  );
  assert.equal(keyCombo(["⌘", "⇧", "K"], "mac"), "⌘⇧K");
  assert.equal(keyCombo(["Ctrl", "Shift", "K"], "windows"), "Ctrl+Shift+K");
  assert.equal(shortcutLabel("link", true), "⌘K");
  assert.equal(shortcutLabel("toolbar", false), "Ctrl+Shift+F");
  setPlatform("linux");
  assert.equal(pick({ mac: 1, windows: 2, linux: 3 }), 3);
  assert.equal(pick({ mac: 1, windows: 2, linux: 3 }, "mac"), 1);
});
