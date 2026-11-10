import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  channelNameIssue, filterChannels, normalizeChannelName, type Filters,
} from "./channel-browser-model.ts";
import type { ChannelRow } from "../api.ts";

const defaults: Filters = {
  query: "", membership: "all", visibility: "all", archived: false, sort: "name",
};
const row = (id: string, extra: Partial<ChannelRow> = {}): ChannelRow => ({
  id, name: id, topic: null, purpose: null, is_member: false, is_private: false,
  is_archived: false, num_members: null, created: null, creator: null, ...extra,
});
const rows = [
  row("a", { is_member: true, num_members: 5, created: 20 }),
  row("b", { topic: "Тема каналу", num_members: 20, created: 10 }),
  row("c", { is_private: true, is_member: true, purpose: "Research", created: 30 }),
  row("d", { is_archived: true }),
];
const ids = (filters: Partial<Filters>) =>
  filterChannels(rows, { ...defaults, ...filters }).map((r) => r.id);

test("channel browser composes membership, visibility, archived and Unicode search filters", () => {
  assert.deepEqual(ids({}), ["a", "b", "c"]);
  assert.deepEqual(ids({ membership: "joined" }), ["a", "c"]);
  assert.deepEqual(ids({ membership: "not_joined" }), ["b"]);
  assert.deepEqual(ids({ visibility: "private" }), ["c"]);
  assert.deepEqual(ids({ visibility: "public", membership: "joined" }), ["a"]);
  assert.deepEqual(ids({ archived: true }), ["a", "b", "c", "d"]);
  assert.deepEqual(ids({ query: " ТЕМА " }), ["b"]);
  assert.deepEqual(ids({ query: "research" }), ["c"]);
  assert.deepEqual(ids({ query: "missing" }), []);
  assert.deepEqual(rows.map((r) => r.id), ["a", "b", "c", "d"]);
});

test("sorts by members/newest with stable name ties and unknown values", () => {
  assert.deepEqual(ids({ sort: "members" }), ["b", "a", "c"]);
  assert.deepEqual(ids({ sort: "newest" }), ["c", "a", "b"]);
});

test("live name preview matches the same Unicode fixtures as Rust validation", () => {
  const cases = JSON.parse(readFileSync(new URL(
    "../../../crates/core/tests/fixtures/channel-names.json", import.meta.url,
  ), "utf8")) as { input: string; normalized: string; valid: boolean }[];
  for (const fixture of cases) {
    const name = normalizeChannelName(fixture.input);
    assert.equal(name, fixture.normalized);
    assert.equal(channelNameIssue(name) === null, fixture.valid, fixture.input);
  }
});
