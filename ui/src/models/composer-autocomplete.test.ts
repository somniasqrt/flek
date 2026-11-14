import test from "node:test";
import assert from "node:assert/strict";
import {
  channelCompletions,
  detectCompletion,
  emojiCompletions,
  personCompletions,
  type ComposerPerson,
} from "./composer-autocomplete.ts";

test("triggers start a word and respect code", () => {
  assert.deepEqual(detectCompletion("hi @al"), {
    kind: "person", query: "al", start: 3, end: 6, complete: false,
  });
  assert.equal(detectCompletion("#")?.kind, "channel");
  assert.equal(detectCompletion("(@")?.kind, "person");
  assert.equal(detectCompletion("привіт @Оле")?.query, "Оле");
  // Mid-word: e-mail, time, issue numbers inside words.
  assert.equal(detectCompletion("mail me@exa"), null);
  assert.equal(detectCompletion("at 10:30"), null);
  assert.equal(detectCompletion("pr#12"), null);
  // Emoji need two characters, a closing colon completes them.
  assert.equal(detectCompletion("ok :s"), null);
  assert.equal(detectCompletion("ok :sm")?.kind, "emoji");
  assert.deepEqual(detectCompletion("ok :smile:"), {
    kind: "emoji", query: "smile", start: 3, end: 10, complete: true,
  });
  assert.equal(detectCompletion("@al:"), null);
  // Inline code and fences, typed but not yet formatted.
  assert.equal(detectCompletion("see `@al"), null);
  assert.equal(detectCompletion("see `x` @al")?.query, "al");
  assert.equal(detectCompletion("```\n@al"), null);
  assert.equal(detectCompletion("```\nx\n```\n@al")?.query, "al");
  assert.equal(detectCompletion("@al", true), null);
});

const person = (p: Partial<ComposerPerson> & { id: string; name: string }): ComposerPerson => ({
  display_name: null, real_name: null, is_bot: false, is_member: false, recent_dm: false, ...p,
});

test("people: prefix before substring, members and DMs first, broadcasts in channels", () => {
  const people = [
    person({ id: "U1", name: "zalina", display_name: "Zalina" }),
    person({ id: "U2", name: "alice", display_name: "Alice" }),
    person({ id: "U3", name: "alex", real_name: "Alex Doe", is_member: true }),
    person({ id: "U4", name: "albert", recent_dm: true }),
    person({ id: "B1", name: "alertbot", is_bot: true }),
  ];
  const ids = (q: string, b = false) => personCompletions(q, people, b).map((c) => c.id);
  assert.deepEqual(ids("al"), ["U3", "U4", "B1", "U2", "U1"]);
  // Bots only when the user typed something.
  assert.ok(!ids("").includes("B1"));
  assert.deepEqual(ids("he", true), ["here"]);
  assert.deepEqual(ids("he", false), []);
  assert.deepEqual(ids("ch", true), ["channel"]);
  const [alex] = personCompletions("doe", people, false);
  assert.deepEqual(alex, { kind: "person", id: "U3", label: "Alex Doe", detail: "Alex Doe" });
});

test("channels: members first, archived hidden", () => {
  const rows = [
    { id: "C1", name: "random-project", is_private: false, is_member: false, is_archived: false },
    { id: "C2", name: "project", is_private: true, is_member: true, is_archived: false },
    { id: "C3", name: "projects-old", is_private: false, is_member: true, is_archived: true },
    { id: "C4", name: "project-x", is_private: false, is_member: false, is_archived: false },
  ];
  assert.deepEqual(channelCompletions("proj", rows).map((c) => c.id), ["C2", "C4", "C1"]);
  assert.deepEqual(channelCompletions("proj", rows)[0], {
    kind: "channel", id: "C2", label: "project", private: true,
  });
});

test("emoji: frequency (all tones) first, then prefix, deduplicated", () => {
  const names = ["smile", "smiley", "grinning_smile", "sm", "smile"];
  const ids = (freq = {}) => emojiCompletions("sm", names, freq).map((c) => c.id);
  assert.deepEqual(ids(), ["sm", "smile", "smiley", "grinning_smile"]);
  assert.deepEqual(ids({ "smiley::skin-tone-2": 2, smiley: 1, smile: 2 }), [
    "smiley", "smile", "sm", "grinning_smile",
  ]);
});
