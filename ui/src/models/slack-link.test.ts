import test from "node:test";
import assert from "node:assert/strict";
import { needsSlack, slackDeepLink } from "./slack-link.ts";

test("Slack channel/message deep links encode every value and omit absent timestamps", () => {
  assert.equal(
    slackDeepLink("T1", "C1", "1700000000.000001"),
    "slack://channel?team=T1&id=C1&message=1700000000.000001",
  );
  assert.equal(slackDeepLink("T1", "D1"), "slack://channel?team=T1&id=D1");
  const link = new URL(slackDeepLink("T&x", "C?x", "1.0&other=bad"));
  assert.equal(link.searchParams.get("team"), "T&x");
  assert.equal(link.searchParams.get("message"), "1.0&other=bad");
  assert.equal(link.searchParams.has("other"), false);
});
test("unknown blocks, attachments, huddles and canvases get the native fallback", () => {
  const base = { subtype: null, files: [], blocks: undefined };
  assert.equal(needsSlack(base), false);
  assert.equal(needsSlack({ ...base, blocks: [{ type: "future" }] }), true);
  assert.equal(needsSlack({ ...base, subtype: "huddle_thread" }), true);
  assert.equal(needsSlack({ ...base, unsupported: true }), true);
});
