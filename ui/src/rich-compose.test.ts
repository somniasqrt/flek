import assert from "node:assert/strict";
import { test } from "node:test";
import { blocksText, safeLink } from "./rich-compose.ts";

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
