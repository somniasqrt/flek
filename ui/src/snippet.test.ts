// Run with `npm test --workspace ui`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { MARK_CLOSE as C, MARK_OPEN as O, pieces, readable } from "./snippet.ts";

test("tokens become readable text", () => {
  assert.equal(
    readable(`Hi <@U1>, see <https://x.example/a|the ${O}plan${C}> in <#C1|general> &gt; ok`, {
      U1: "Olena",
    }),
    `Hi @Olena, see the ${O}plan${C} in #general > ok`,
  );
  assert.equal(readable("<!here> <https://x.example>\n\nnext"), "@here https://x.example next");
});

test("emphasis and emoji", () => {
  const emoji = (n: string) => (n === "wave" ? "👋" : undefined);
  assert.equal(
    readable(`Hi :wave: it is *${O}online${C}* _now_, ~not~ snake_case_name :custom:`, {}, emoji),
    `Hi 👋 it is ${O}online${C} now, not snake_case_name :custom:`,
  );
});

test("a match inside a hidden URL marks the whole token", () => {
  assert.equal(readable(`<https://${O}zoom${C}.us/j/1|Join>`), `${O}Join${C}`);
});

test("pieces split on markers, Cyrillic included", () => {
  assert.deepEqual(pieces(`Таблиця ${O}оцінок${C} тут`), [
    { text: "Таблиця ", mark: false },
    { text: "оцінок", mark: true },
    { text: " тут", mark: false },
  ]);
  // An unclosed marker runs to the end; a stray close is dropped.
  assert.deepEqual(pieces(`a${C}b ${O}c`), [
    { text: "a", mark: false },
    { text: "b ", mark: false },
    { text: "c", mark: true },
  ]);
});
