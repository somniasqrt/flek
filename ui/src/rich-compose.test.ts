import assert from "node:assert/strict";
import { test } from "node:test";
import { blocksText, safeLink, linkify } from "./rich-compose.ts";

test("rich text has a searchable plain fallback, including mentions and lists", () => {
  assert.equal(
    blocksText([
      {
        type: "rich_text",
        elements: [
          {
            type: "rich_text_section",
            elements: [
              { type: "text", text: "Hello ", style: { bold: true } },
              { type: "user", user_id: "U1" },
            ],
          },
          {
            type: "rich_text_list",
            style: "ordered",
            indent: 0,
            elements: [
              {
                type: "rich_text_section",
                elements: [{ type: "link", url: "https://example.com", text: "Example" }],
              },
            ],
          },
        ],
      },
    ]),
    "Hello @U1\nExample",
  );
});

test("editor links accept safe protocols only", () => {
  for (const url of ["https://example.com", "http://localhost:1234", "mailto:test@example.com"])
    assert.equal(safeLink(url), true);
  for (const url of [
    "javascript:alert(1)",
    "data:text/html,hello",
    "file:///etc/passwd",
    "not a url",
  ])
    assert.equal(safeLink(url), false);
});

const t = (text: string) => ({ type: "text", text });
const l = (url: string) => ({ type: "link", url });
test("linkify: URL in a sentence and trailing punctuation", () => {
  assert.deepEqual(linkify("See https://example.com/a.,;:!? now"), [t("See "), l("https://example.com/a"), t(".,;:!? now")]);
});
test("linkify: parentheses are retained only when balanced inside the URL", () => {
  assert.deepEqual(linkify("(https://example.com/a)"), [t("("), l("https://example.com/a"), t(")")]);
  assert.deepEqual(linkify("(https://example.com/a_(b))."), [t("("), l("https://example.com/a_(b)"), t(").")]);
});
test("linkify: two URLs preserve styles and surrounding text", () => {
  assert.deepEqual(linkify("https://a.example and http://b.example", { bold: true }), [
    { ...l("https://a.example"), style: { bold: true } },
    { ...t(" and "), style: { bold: true } },
    { ...l("http://b.example"), style: { bold: true } },
  ]);
});
test("linkify: code remains untouched", () => {
  assert.deepEqual(linkify("https://example.com", { code: true }), [{ ...t("https://example.com"), style: { code: true } }]);
});
