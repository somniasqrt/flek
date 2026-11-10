import test from "node:test";
import assert from "node:assert/strict";
import { firstUnread, conversationActions, hasUnread } from "./conversation-model.ts";

test("New divider uses the opening marker and crosses day/page boundaries", () => {
  const messages = [{ ts: "100.000001" }, { ts: "100.000002" }, { ts: "101.000001" }];
  const snapshot = "100.000001";
  assert.equal(firstUnread(messages, snapshot), "100.000002");
  assert.equal(firstUnread(messages, "101.000001"), null);
  assert.equal(firstUnread(messages, null), null);
  assert.equal(firstUnread([], snapshot), null);
});

test("channel and DM utilities", () => {
  assert.equal(conversationActions("public_channel").dismiss, "Leave channel");
  assert.equal(conversationActions("private_channel").details, "Channel details");
  for (const kind of ["im", "mpim"]) {
    assert.equal(conversationActions(kind).channel, false);
    assert.equal(conversationActions(kind).dismiss, "Close conversation");
  }
});

test("muted conversations never contribute unread badges", () => {
  assert.equal(hasUnread({ unread: 4, notification: "muted" }), false);
  assert.equal(hasUnread({ unread: 4, notification: "mentions" }), true);
  assert.equal(hasUnread({ unread: null }), false);
});
