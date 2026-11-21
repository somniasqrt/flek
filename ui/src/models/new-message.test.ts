import assert from "node:assert/strict";
import test from "node:test";
import type { Conversation } from "../api";
import type { ComposerPerson } from "./composer-autocomplete.ts";
import { MAX_DM_PEOPLE, newMessageRows } from "./new-message.ts";

const person = (p: Partial<ComposerPerson> & { id: string; name: string }): ComposerPerson => ({
  display_name: null,
  real_name: null,
  is_bot: false,
  is_member: false,
  recent_dm: false,
  ...p,
});

const conv = (
  c: Partial<Conversation> & { id: string; kind: Conversation["kind"]; label: string },
): Conversation => ({
  is_archived: false,
  topic: null,
  messages: 0,
  unread: 0,
  latest_ts: null,
  dm_user_id: null,
  ...c,
});

const people: ComposerPerson[] = [
  person({ id: "U1", name: "me", display_name: "Me", real_name: "Me User" }),
  person({ id: "U2", name: "alice", display_name: "Alice", real_name: "Alice Smith", recent_dm: true }),
  person({ id: "U3", name: "bob", display_name: "Bob", real_name: "Bob Jones" }),
  person({ id: "U4", name: "carol", display_name: "Carol" }),
  person({ id: "UBOT", name: "helper", display_name: "Helper Bot", is_bot: true }),
];

const conversations: Conversation[] = [
  conv({ id: "C1", kind: "public_channel", label: "general" }),
  conv({ id: "C2", kind: "private_channel", label: "secret" }),
  conv({ id: "C3", kind: "public_channel", label: "old-general", is_archived: true }),
  conv({ id: "D2", kind: "im", label: "Alice", dm_user_id: "U2" }),
  conv({ id: "DBOT", kind: "im", label: "Helper Bot", dm_user_id: "UBOT" }),
  conv({ id: "G1", kind: "mpim", label: "Alice, Bob" }),
];

test("dedupes im conversations when matching person is shown and hides bots and archived", () => {
  const rows = newMessageRows("", [], people, conversations, "U1");
  // Alice's im (D2) is hidden because U2 is shown as a person row.
  assert.ok(!rows.some((r) => r.kind === "conversation" && r.id === "D2"));
  assert.ok(rows.some((r) => r.kind === "person" && r.id === "U2"));
  // Archived channels are hidden.
  assert.ok(!rows.some((r) => r.id === "C3"));
  // Bots are excluded from person rows (existing bot DM conversation stays available).
  assert.ok(!rows.some((r) => r.kind === "person" && r.id === "UBOT"));
  assert.ok(rows.some((r) => r.kind === "conversation" && r.id === "DBOT"));
  // Self is allowed when no one is selected yet (self-DM).
  assert.ok(rows.some((r) => r.kind === "person" && r.id === "U1"));
  // Channels and group DMs remain present.
  assert.ok(rows.some((r) => r.kind === "conversation" && r.id === "C1"));
  assert.ok(rows.some((r) => r.kind === "conversation" && r.id === "G1"));
});

test("dedupes im by label when dm_user_id is null", () => {
  const noDmUserId = [conv({ id: "D_LABEL", kind: "im", label: "Carol", dm_user_id: null })];
  const rows = newMessageRows("carol", [], people, noDmUserId, "U1");
  assert.deepEqual(
    rows.map((r) => [r.kind, r.id]),
    [["person", "U4"]],
  );
});

test("supports @ and # prefixes", () => {
  const atRows = newMessageRows("@al", [], people, conversations, "U1");
  assert.deepEqual(
    atRows.map((r) => [r.kind, r.id]),
    [["person", "U2"]],
  );
  const hashRows = newMessageRows("#", [], people, conversations, "U1");
  assert.deepEqual(
    hashRows.map((r) => [r.kind, r.id]),
    [
      ["conversation", "C1"],
      ["conversation", "C2"],
    ],
  );
  // With a person already selected, # yields nothing because only people can be added.
  assert.deepEqual(
    newMessageRows("#gen", [{ id: "U2", label: "Alice" }], people, conversations, "U1"),
    [],
  );
});

test("when people are selected, shows only unselected people and excludes own user id", () => {
  const rows = newMessageRows("", [{ id: "U2", label: "Alice" }], people, conversations, "U1");
  assert.ok(rows.every((r) => r.kind === "person"));
  const ids = rows.map((r) => r.id);
  assert.ok(!ids.includes("U2"), "already selected person is hidden");
  assert.ok(!ids.includes("U1"), "own user id is excluded once others are selected");
  assert.ok(!ids.includes("UBOT"), "bots remain excluded");
  assert.deepEqual(ids.sort(), ["U3", "U4"]);
});

test("stops returning rows once 8 people are selected and caps output at 60 rows", () => {
  const eight = Array.from({ length: MAX_DM_PEOPLE }, (_, i) => ({
    id: `UX${i}`,
    label: `Person ${i}`,
  }));
  assert.deepEqual(newMessageRows("", eight, people, conversations, "U1"), []);

  const manyPeople = Array.from({ length: 80 }, (_, i) =>
    person({ id: `UP${i}`, name: `user${i}`, display_name: `User ${i}` }),
  );
  assert.equal(newMessageRows("", [], manyPeople, [], "U1").length, 60);
});
