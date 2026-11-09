import assert from "node:assert/strict";
import { test } from "node:test";
import { editShortcut, isMacPlatform, shortcutLabel } from "./shortcuts.ts";
const event = (extra = {}) => ({
  code: "KeyB",
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  isComposing: false,
  ...extra,
});

test("macOS uses Command; Windows and Linux use Control", () => {
  assert.equal(editShortcut(event({ metaKey: true }), true), "bold");
  assert.equal(editShortcut(event({ ctrlKey: true }), false), "bold");
  assert.equal(editShortcut(event({ ctrlKey: true }), true), undefined);
  assert.equal(editShortcut(event({ metaKey: true }), false), undefined);
  assert.equal(isMacPlatform("MacIntel"), true);
  assert.equal(isMacPlatform("Win32"), false);
  assert.equal(isMacPlatform("Linux x86_64"), false);
});

test("physical codes work independently of the typed character", () => {
  assert.equal(editShortcut(event({ code: "KeyI", ctrlKey: true }), false), "italic");
  assert.equal(
    editShortcut(event({ code: "Digit7", ctrlKey: true, shiftKey: true }), false),
    "ordered",
  );
  assert.equal(
    editShortcut(event({ code: "KeyX", metaKey: true, shiftKey: true }), true),
    "strike",
  );
});

test("IME and AltGr never trigger formatting, ordinary copy is untouched", () => {
  assert.equal(editShortcut(event({ ctrlKey: true, altKey: true }), false), undefined);
  assert.equal(editShortcut(event({ metaKey: true, isComposing: true }), true), undefined);
  assert.equal(editShortcut(event({ code: "KeyC", ctrlKey: true }), false), undefined);
  assert.equal(shortcutLabel("bold", true), "⌘B");
  assert.equal(shortcutLabel("pre", false), "Ctrl+Shift+J");
});
