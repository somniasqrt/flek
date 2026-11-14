import test from "node:test";
import assert from "node:assert/strict";
import {
  firstUnread,
  conversationActions,
  conversationMenuPosition,
  conversationStarLabel,
  hasUnread,
} from "./conversation-model.ts";

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

test("header star label names channels and other conversations", () => {
  assert.equal(conversationStarLabel({ kind: "public_channel" }), "Star channel");
  assert.equal(conversationStarLabel({ kind: "private_channel", starred: true }), "Unstar channel");
  assert.equal(conversationStarLabel({ kind: "im" }), "Star conversation");
  assert.equal(conversationStarLabel({ kind: "mpim", starred: true }), "Unstar conversation");
});

test("header menus hang from their right edge and stay inside the window", () => {
  // Header: right edge under the kebab.
  assert.deepEqual(conversationMenuPosition(900, 50, 240, 300, 1000, 800, true), { left: 660, top: 50 });
  // Sidebar: left edge at the pointer.
  assert.deepEqual(conversationMenuPosition(100, 50, 240, 300, 1000, 800, false), { left: 100, top: 50 });
  // Clamped at every edge with an 8px margin.
  assert.deepEqual(conversationMenuPosition(100, 50, 240, 300, 1000, 800, true), { left: 8, top: 50 });
  assert.deepEqual(conversationMenuPosition(990, 700, 240, 300, 1000, 800, false), { left: 752, top: 492 });
  assert.deepEqual(conversationMenuPosition(-5, -5, 240, 300, 1000, 800, false), { left: 8, top: 8 });
});
