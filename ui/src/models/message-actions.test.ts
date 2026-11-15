import assert from "node:assert/strict";
import { test } from "node:test";
import { messageShortcut } from "./message-actions.ts";

const event = (code: string, extra = {}) => ({
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  isComposing: false,
  ...extra,
});

test("bare letters map to message actions by physical key", () => {
  const expected = {
    KeyR: "reaction",
    KeyT: "thread",
    KeyF: "forward",
    KeyU: "unread",
    KeyP: "pin",
    KeyL: "link",
  };
  for (const mac of [true, false]) {
    for (const [code, action] of Object.entries(expected)) {
      assert.equal(messageShortcut(event(code), mac), action, code);
    }
  }
  assert.equal(messageShortcut(event("KeyZ"), true), undefined);
});

test("copy and save need the platform primary modifier", () => {
  assert.equal(messageShortcut(event("KeyC", { metaKey: true }), true), "copy");
  assert.equal(messageShortcut(event("KeyC", { ctrlKey: true }), false), "copy");
  assert.equal(messageShortcut(event("KeyS", { metaKey: true, shiftKey: true }), true), "save");
  assert.equal(messageShortcut(event("KeyS", { ctrlKey: true, shiftKey: true }), false), "save");
  // Wrong modifier for the platform, or plain S / Shift+C, does nothing.
  assert.equal(messageShortcut(event("KeyC", { ctrlKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyC", { metaKey: true }), false), undefined);
  assert.equal(messageShortcut(event("KeyS", { metaKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyC", { metaKey: true, shiftKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyS"), true), undefined);
});

test("modified letters, Alt and IME composition never trigger actions", () => {
  assert.equal(messageShortcut(event("KeyF", { metaKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyF", { ctrlKey: true }), false), undefined);
  assert.equal(messageShortcut(event("KeyF", { shiftKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyP", { altKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyC", { metaKey: true, altKey: true }), true), undefined);
  assert.equal(messageShortcut(event("KeyR", { isComposing: true }), true), undefined);
});
