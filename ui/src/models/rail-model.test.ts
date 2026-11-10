import test from "node:test";
import assert from "node:assert/strict";
import { activityDay as formatActivityDay, listTime as formatListTime } from "./time.ts";
import { fileType, messagePreview, readRail } from "./rail-model.ts";

// The English fixture uses day/month order explicitly; UI defaults now follow locale.
const listTime = (date: Date, now: Date) => formatListTime(date, now, "en-GB");
const activityDay = (date: Date, now: Date) => formatActivityDay(date, now, "en-GB");

const now = new Date(2026, 10, 10, 14, 0);

test("compact list times use minutes, hours, weekdays, dates and years", () => {
  assert.equal(listTime(new Date(2026, 10, 10, 13, 50), now), "10 mins");
  assert.equal(listTime(new Date(2026, 10, 10, 12), now), "2 hours");
  assert.equal(listTime(new Date(2026, 10, 9, 9), now), "Monday");
  assert.equal(listTime(new Date(2026, 9, 26), now), "26 October");
  assert.equal(listTime(new Date(2025, 9, 26), now), "26 October 2025");
  assert.equal(listTime(now, now), "Just now");
});

test("activity headers are calendar days, including yesterday and year boundaries", () => {
  assert.equal(activityDay(now, now), "Today");
  assert.equal(activityDay(new Date(2026, 10, 9), now), "Yesterday");
  assert.equal(activityDay(new Date(2026, 10, 8), now), "Sunday");
  assert.equal(activityDay(new Date(2026, 9, 26), now), "26 October");
  assert.equal(activityDay(new Date(2025, 9, 26), now), "26 October 2025");
  assert.equal(activityDay(new Date(2025, 11, 31), new Date(2026, 0, 1)), "Yesterday");
});

test("DM previews resolve markup, prefix mine and preserve archived text", () => {
  const row = {
    text: "Hi <@U2> *friend*",
    user_id: "ME",
    file_name: null,
    hidden_by_limit: false,
  };
  assert.equal(messagePreview(row, "ME", { U2: "Alice" }), "You: Hi @Alice friend");
  assert.equal(
    messagePreview({ ...row, text: "", file_name: "plan.pdf" }, "ME", {}),
    "You: plan.pdf",
  );
  assert.equal(
    messagePreview({ ...row, text: "", user_id: "U2", file_name: "old.pdf", hidden_by_limit: true }, "ME", {}),
    "Hidden by Slack's 90-day limit",
  );
  assert.equal(
    messagePreview({ ...row, hidden_by_limit: true }, "ME", { U2: "Alice" }),
    "You: Hi @Alice friend",
  );
});

test("file categories match the core filters", () => {
  for (const mime of ["image/png", "image/jpeg"]) assert.equal(fileType(mime), "image");
  assert.equal(fileType("application/pdf"), "pdf");
  for (const mime of [
    "text/plain",
    "application/msword",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  ]) {
    assert.equal(fileType(mime), "doc");
  }
  for (const mime of [null, "application/zip", "video/mp4"]) assert.equal(fileType(mime), "other");
});

test("rail preferences tolerate missing or blocked localStorage", () => {
  assert.equal(readRail("missing"), "home");
});
