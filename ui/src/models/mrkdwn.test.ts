// Run with `npm test --workspace ui` (Node's own test runner, types stripped).
import assert from "node:assert/strict";
import { test } from "node:test";
import { inline, parse, plainText } from "./mrkdwn.ts";

test("emphasis, code and escapes", () => {
  assert.deepEqual(inline("*bold* and _it_ ~gone~ `a<b` &lt;3"), [
    { t: "b", c: [{ t: "text", v: "bold" }] },
    { t: "text", v: " and " },
    { t: "i", c: [{ t: "text", v: "it" }] },
    { t: "text", v: " " },
    { t: "s", c: [{ t: "text", v: "gone" }] },
    { t: "text", v: " " },
    { t: "code", v: "a<b" },
    { t: "text", v: " <3" },
  ]);
  // Not emphasis: snake_case words, a lone star, 2 * 3 * 4.
  assert.deepEqual(inline("snake_case_name"), [{ t: "text", v: "snake_case_name" }]);
  assert.deepEqual(inline("2 * 3 * 4"), [{ t: "text", v: "2 * 3 * 4" }]);
});

test("Slack tokens", () => {
  assert.deepEqual(inline("hi <@U1> in <#C1|general> <!here>"), [
    { t: "text", v: "hi " },
    { t: "user", id: "U1", label: undefined },
    { t: "text", v: " in " },
    { t: "channel", id: "C1", label: "general" },
    { t: "text", v: " " },
    { t: "special", v: "@here" },
  ]);
  assert.deepEqual(inline("<https://zoom.us/j/1|Join *now*>"), [
    { t: "link", url: "https://zoom.us/j/1", label: [{ t: "text", v: "Join " }, { t: "b", c: [{ t: "text", v: "now" }] }] },
  ]);
  assert.deepEqual(inline("see https://example.com/a."), [
    { t: "text", v: "see " },
    { t: "link", url: "https://example.com/a" },
    { t: "text", v: "." },
  ]);
  assert.deepEqual(inline(":+1::skin-tone-3: :custom-one:"), [
    { t: "emoji", name: "+1::skin-tone-3" },
    { t: "text", v: " " },
    { t: "emoji", name: "custom-one" },
  ]);
});

test("copied text comes from the stored message, with full links", () => {
  const url = "https://docs.google.com/document/d/1bSDg6PV3GGmFAKEFAKEFAKE/edit?usp=sharing&amp;x=1";
  assert.equal(
    plainText(`Пані і панове, кидаю вимоги до лаб:\n<${url}>`),
    "Пані і панове, кидаю вимоги до лаб:\nhttps://docs.google.com/document/d/1bSDg6PV3GGmFAKEFAKEFAKE/edit?usp=sharing&x=1",
  );
  assert.equal(
    plainText("*Hi* <@U1> and <@U2|bob> in <#C1|general> <!here> :wave: <https://example.test|site> <https://a.test|https://a.test> <mailto:a@b.test|a@b.test> `x &lt; y`", (id) =>
      id === "U1" ? "Ali" : undefined),
    "Hi @Ali and @bob in #general @here :wave: site (https://example.test) https://a.test a@b.test x < y",
  );
  assert.equal(
    plainText("- one\n- two\n1. first\n&gt; quoted\n```let x = 1;\n```"),
    "• one\n• two\n1. first\n> quoted\nlet x = 1;",
  );
});

test("blocks: headings, lists, quotes, code", () => {
  const blocks = parse("# Title\n- one\n- two\n  - nested\n1. first\n2. second\n&gt; quoted\ntext\n```let x = 1;\n```");
  assert.deepEqual(
    blocks.map((b) => b.t),
    ["h", "list", "list", "quote", "p", "pre"],
  );
  const bullets = blocks[1] as Extract<(typeof blocks)[number], { t: "list" }>;
  assert.deepEqual(bullets.items.map((i) => i.level), [0, 0, 1]);
  assert.equal((blocks[2] as { ordered: boolean }).ordered, true);
  assert.equal((blocks[5] as { v: string }).v, "let x = 1;");
  // "*bold* line" is not a list item.
  assert.deepEqual(parse("*bold* line").map((b) => b.t), ["p"]);
  // An unclosed fence stays text.
  assert.deepEqual(parse("a ``` b").map((b) => b.t), ["p"]);
});
