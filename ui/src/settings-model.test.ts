import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultSettings, messageSize, sortDms } from "./settings-model.ts";

test("preferences defaults and non-mutating DM ordering", () => {
  assert.equal(defaultSettings.max_file_bytes, 100 << 20);
  assert.equal(defaultSettings.refresh_days, 7);
  assert.equal(defaultSettings.refresh_hours, 3);
  assert.equal(defaultSettings.clock_24, null);
  const rows = [
    { label: "Zoe", ts: "20" },
    { label: "Anna", ts: "10" },
  ];
  assert.deepEqual(
    sortDms(rows, "name", "en").map((r) => r.label),
    ["Anna", "Zoe"],
  );
  assert.deepEqual(
    sortDms(rows, "recent", "en").map((r) => r.label),
    ["Zoe", "Anna"],
  );
  assert.equal(rows[0].label, "Zoe");
  assert.equal(messageSize("large"), "18px");
  assert.equal(messageSize("small"), "13px");
});
