import test from "node:test";
import assert from "node:assert/strict";
import type { Conversation, DirectMessage } from "../api";
import { rankConversations, adjacentConversation, conversationShortcut } from "./switcher.ts";
import { sidebarOrder } from "./sidebar-navigation.ts";
const conv = (id: string, label: string, extra: Partial<Conversation> = {}): Conversation => ({
  id,
  label,
  kind: "public_channel",
  is_archived: false,
  topic: null,
  messages: 0,
  unread: 0,
  latest_ts: null,
  dm_user_id: null,
  ...extra,
});
test("switcher matches prefixes, fuzzy names, Unicode and DM member display names", () => {
  const rows = [
    conv("C1", "project-alpha"),
    conv("D1", "Group", { kind: "mpim" }),
    conv("C2", "Розробка"),
  ];
  const dms = [{ id: "D1", members: ["U1", "U2"] }] as DirectMessage[];
  assert.equal(rankConversations(rows, "pral")[0].id, "C1");
  assert.equal(rankConversations(rows, "#роз")[0].id, "C2");
  assert.equal(rankConversations(rows, "@elodie", dms, { U1: "Élodie Test" })[0].id, "D1");
  assert.deepEqual(rankConversations(rows, "absent"), []);
});
test("match quality comes first, then unread and recent; archived are excluded", () => {
  const rows = [
    conv("C1", "dev", { latest_ts: "100" }),
    conv("C2", "dev-team", { unread: 2 }),
    conv("C3", "dev-tools", { latest_ts: "200" }),
    conv("C4", "dev-old", { is_archived: true }),
  ];
  assert.deepEqual(
    rankConversations(rows, "").map((c) => c.id),
    ["C2", "C3", "C1"],
  );
  assert.deepEqual(
    rankConversations(rows, "dev").map((c) => c.id),
    ["C1", "C2", "C3"],
  );
});
test("navigation uses complete sidebar order, wraps, skips muted unread, and handles no selection", () => {
  const rows = sidebarOrder(
    [
      conv("C2", "z"),
      conv("C1", "a", { unread: 1 }),
      conv("D1", "Person", { kind: "im", starred: true }),
      conv("C3", "muted", { unread: 4, notification: "muted" }),
    ],
    "recent",
    "en",
    true,
  );
  assert.deepEqual(
    rows.map((c) => c.id),
    ["D1", "C1", "C2", "C3"],
  );
  assert.equal(adjacentConversation(rows, "D1", 1, true)?.id, "C1");
  assert.equal(adjacentConversation(rows, "C1", 1, true), undefined);
  assert.equal(adjacentConversation(rows, "D1", -1)?.id, "C3");
  assert.equal(adjacentConversation(rows, null, 1)?.id, "D1");
  assert.equal(adjacentConversation(rows, null, -1)?.id, "C3");
  assert.equal(adjacentConversation([], null, 1), undefined);
});
test("shortcuts preserve input editing, IME and AltGr; primary K works on physical key", () => {
  const e = {
    code: "ArrowUp",
    metaKey: false,
    ctrlKey: false,
    altKey: true,
    shiftKey: true,
    isComposing: false,
  };
  assert.equal(conversationShortcut(e, true, false), "previous-unread");
  assert.equal(conversationShortcut(e, true, true), undefined);
  assert.equal(conversationShortcut({ ...e, ctrlKey: true }, false, false), undefined);
  assert.equal(conversationShortcut({ ...e, isComposing: true }, true, false), undefined);
  assert.equal(
    conversationShortcut(
      { ...e, code: "KeyK", altKey: false, shiftKey: false, metaKey: true },
      true,
      true,
    ),
    "switcher",
  );
});
