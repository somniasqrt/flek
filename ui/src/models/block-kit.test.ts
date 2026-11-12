import test from "node:test";
import assert from "node:assert/strict";
import { parseBlockKit } from "./block-kit.ts";
const plain = { type: "plain_text", text: "<script>literal</script>" };
const image = { type: "image", image_url: "https://example.test/image.png", alt_text: "Preview" };
test("read-only Block Kit preserves text kinds, fields, images and disabled button labels", () => {
  const raw = [
    {
      type: "section",
      text: { type: "mrkdwn", text: "*hello*" },
      fields: [plain],
      accessory: image,
    },
    { type: "context", elements: [plain, image] },
    image,
    { type: "header", text: plain },
    { type: "divider" },
    {
      type: "actions",
      elements: [{ type: "button", text: plain, url: "javascript:evil()", action_id: "never-run" }],
    },
    {
      type: "rich_text",
      elements: [{ type: "rich_text_section", elements: [{ type: "text", text: "rich" }] }],
    },
  ];
  const parsed = parseBlockKit(raw);
  assert.equal(parsed.fallback, false);
  assert.equal(parsed.blocks.length, 7);
  const actions = parsed.blocks[5];
  assert.equal(actions.type, "actions");
  assert.deepEqual(actions, { type: "actions", elements: [{ text: plain }] });
});
test("unknown, mixed, malformed, null and deeply nested blocks fall back without throwing", () => {
  for (const raw of [
    {},
    "broken",
    [null],
    [{ type: "video" }],
    [{ type: "divider" }, { type: "future" }],
    [{ type: "section", fields: "bad" }],
    [{ type: "section", text: plain, accessory: { type: "button" } }],
    [{ type: "actions", elements: [{ type: "static_select" }] }],
    [{ type: "context", elements: [null] }],
  ]) {
    assert.deepEqual(parseBlockKit(raw), { blocks: [], fallback: true });
  }
  let nested: unknown = { type: "text", text: "deep" };
  for (let i = 0; i < 8; i++) nested = { type: "rich_text_section", elements: [nested] };
  assert.equal(parseBlockKit([{ type: "rich_text", elements: [nested] }]).fallback, true);
  assert.deepEqual(parseBlockKit(null), { blocks: [], fallback: false });
  assert.deepEqual(parseBlockKit([]), { blocks: [], fallback: false });
});
