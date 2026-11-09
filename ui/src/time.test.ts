import assert from "node:assert/strict";
import { test } from "node:test";
import { relativeTime } from "./time.ts";

test("last reply labels", () => {
  const now = new Date(2026, 10, 9, 15, 30);
  const at = (...a: [number, number, number, number, number]) => relativeTime(new Date(...a), now, "en");
  assert.equal(at(2026, 10, 9, 15, 29), "1 minute ago");
  assert.equal(relativeTime(new Date(now.getTime() - 20_000), now, "en"), "just now");
  assert.equal(at(2026, 10, 9, 15, 5), "25 minutes ago");
  assert.match(at(2026, 10, 9, 9, 7), /^today at 0?9:07/);
  assert.match(at(2026, 10, 8, 23, 50), /^yesterday at 11:50 PM|^yesterday at 23:50/);
  assert.equal(at(2026, 10, 6, 12, 0), "3 days ago");
  assert.equal(at(2026, 8, 1, 12, 0), "2 months ago");
  assert.equal(at(2024, 9, 1, 12, 0), "2 years ago");
});
