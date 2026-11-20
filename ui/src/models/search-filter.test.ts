import test from "node:test";
import assert from "node:assert/strict";
import { applyFilter, detectFilter, filterSuggestions } from "./search-filter.ts";
import type { ComposerPerson } from "./composer-autocomplete.ts";

const person = (id: string, name: string, display: string | null = null, extra: Partial<ComposerPerson> = {}): ComposerPerson => ({
  id,
  name,
  display_name: display,
  real_name: null,
  is_bot: false,
  is_member: false,
  recent_dm: false,
  ...extra,
});
const people = [person("U1", "alex", "Alex Ko"), person("U2", "alina"), person("U3", "bob", "alex")];
const conversations = [
  { id: "C1", kind: "public_channel" as const, label: "general", is_archived: false },
  { id: "C2", kind: "private_channel" as const, label: "gen-secret", is_archived: false },
  { id: "C3", kind: "public_channel" as const, label: "gen-old", is_archived: true },
  { id: "D1", kind: "im" as const, label: "Genia Petrenko", is_archived: false },
  { id: "C4", kind: "public_channel" as const, label: "random", is_archived: false },
];

test("detects the filter under the caret, including text after it", () => {
  assert.deepEqual(detectFilter("grades in:#gen", 14), { kind: "in", query: "#gen", start: 7, end: 14 });
  assert.deepEqual(detectFilter("from:al draft", 7), { kind: "from", query: "al", start: 0, end: 7 });
  assert.deepEqual(detectFilter("from:alex draft", 7), { kind: "from", query: "alex", start: 0, end: 9 });
  assert.deepEqual(detectFilter("FROM:", 5), { kind: "from", query: "", start: 0, end: 5 });
  assert.equal(detectFilter("within:x", 8), null);
  assert.equal(detectFilter("in:#gen ", 8), null, "a finished filter is not reopened");
  assert.equal(detectFilter("has:link", 8), null);
  assert.equal(detectFilter('in:"x', 5), null);
});

test("channels first, archived hidden, sigils restrict the kind", () => {
  const ids = (q: string) =>
    filterSuggestions({ kind: "in", query: q, start: 0, end: 0 }, conversations, people).map((s) => s.id);
  assert.deepEqual(ids("gen"), ["C2", "C1", "D1"], "channels by name, then DMs; archived hidden");
  assert.deepEqual(ids("#gen"), ["C2", "C1"]);
  assert.deepEqual(ids("@gen"), ["D1"]);
});

test("in: inserts #name for channels and the id for DMs", () => {
  const s = filterSuggestions({ kind: "in", query: "", start: 0, end: 0 }, conversations, people, 10);
  const insert = Object.fromEntries(s.map((x) => [x.id, x.insert]));
  assert.equal(insert.C1, "in:#general ");
  assert.equal(insert.D1, "in:D1 ");
  assert.equal(s.find((x) => x.id === "C2")?.private, true);
});

test("from: uses the handle unless another person shares it as a name", () => {
  const s = filterSuggestions({ kind: "from", query: "al", start: 0, end: 0 }, conversations, people);
  const insert = Object.fromEntries(s.map((x) => [x.id, x.insert]));
  assert.equal(insert.U2, "from:@alina ");
  assert.equal(insert.U1, "from:U1 ", "bob's display name is alex");
  assert.deepEqual(filterSuggestions({ kind: "from", query: "#al", start: 0, end: 0 }, conversations, people), []);
});

test("from: hides bots until something is typed", () => {
  const bots = [person("B1", "deploybot", null, { is_bot: true })];
  assert.deepEqual(filterSuggestions({ kind: "from", query: "", start: 0, end: 0 }, [], bots), []);
  assert.equal(filterSuggestions({ kind: "from", query: "dep", start: 0, end: 0 }, [], bots).length, 1);
});

test("accepting replaces the whole token and keeps the rest of the query", () => {
  const text = "grades in:#ge draft";
  const token = detectFilter(text, 13)!;
  assert.deepEqual(applyFilter(text, token, "in:#general "), { text: "grades in:#general draft", caret: 19 });
  const end = detectFilter("from:al", 7)!;
  assert.deepEqual(applyFilter("from:al", end, "from:@alina "), { text: "from:@alina ", caret: 12 });
});
