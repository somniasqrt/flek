import assert from "node:assert/strict";
import { test } from "node:test";
import { parse } from "./mrkdwn.ts";
import { previewBlocks, previewText } from "./unfurl-text.ts";

test("Facebook preview decodes Ukrainian, numeric emoji and named punctuation", () => {
  assert.equal(
    previewText(
      "&#127757; ML WEEK 2025 &ndash; &#1050;&#1055;&#1030; &laquo;&#1053;&#1058;&#1059;&raquo; &#x1F4BB;",
    ),
    "🌍 ML WEEK 2025 – КПІ «НТУ» 💻",
  );
  assert.equal(
    previewText("&copy; &quot;quoted&quot; &amp; &#39;text&#39;"),
    "© \"quoted\" & 'text'",
  );
});

test("preview formatting and actual Slack links survive decoding", () => {
  const text =
    "*&#1050;&#1055;&#1030;* <https://example.com/?x=1&amp;y=2|&#1050;&#1091;&#1088;&#1089;>";
  assert.deepEqual(previewBlocks(parse(text)), [
    {
      t: "p",
      lines: [
        [
          { t: "b", c: [{ t: "text", v: "КПІ" }] },
          { t: "text", v: " " },
          { t: "link", url: "https://example.com/?x=1&y=2", label: [{ t: "text", v: "Курс" }] },
        ],
      ],
    },
  ]);
});

test("encoded markup stays literal and ordinary messages keep Slack escape rules", () => {
  assert.deepEqual(
    previewBlocks(parse("&#60;@U1&#62; &#60;script&#62;alert(1)&#60;/script&#62;")),
    [{ t: "p", lines: [[{ t: "text", v: "<@U1> <script>alert(1)</script>" }]] }],
  );
  assert.deepEqual(parse("&#1050; &lt;@U1&gt;"), [
    { t: "p", lines: [[{ t: "text", v: "&#1050; <@U1>" }]] },
  ]);
  assert.equal(
    previewText("unknown &notAnEntity; &ndash without semicolon"),
    "unknown &notAnEntity; &ndash without semicolon",
  );
});
