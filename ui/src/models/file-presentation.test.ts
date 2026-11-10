import test from "node:test";
import assert from "node:assert/strict";
import { canOpen, fileName } from "./file-presentation.ts";

test("real name wins over a misleading sender title and bidi controls", () => {
  const file = { name: "x.terminal", title: "Розклад.pdf" };
  assert.equal(fileName(file, "file"), "x.terminal");
  assert.equal(fileName({ name: "Розклад\u202efdp.command" }, "file"), "Розкладfdp.command");
  assert.equal(fileName({ name: null }, "file"), "file");
});
test("UI uses core launch decision and fails closed for missing decisions", () => {
  assert.equal(canOpen({ name: "x.pdf", can_open: true }), true);
  assert.equal(canOpen({ name: "x.terminal", can_open: false }), false);
  assert.equal(canOpen({ name: "x.pdf" }), false);
});
