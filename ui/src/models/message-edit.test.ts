import assert from "node:assert/strict";
import { test } from "node:test";
import { messageActionKey, messageEditDraft } from "./message-edit.ts";

const html = (draft: string | null) => {
  assert.notEqual(draft, null);
  const parsed = JSON.parse(draft!);
  assert.equal(parsed.version, 1);
  return parsed.html as string;
};
const section = (...elements: unknown[]) => ({ type: "rich_text_section", elements });
const rich = (...elements: unknown[]) => [{ type: "rich_text", elements }];

test("plain mrkdwn becomes a safe draft with mention atoms and escaped text", () => {
  assert.equal(
    html(messageEditDraft("hi *bold* <@U1> a &amp; b &lt;script&gt;", undefined, { U1: "Olena" })),
    '<div>hi <b>bold</b> <span contenteditable="false" data-user-id="U1">@Olena</span> a &amp; b &lt;script&gt;</div>',
  );
});

test("rich text blocks round trip styles, mentions and links", () => {
  const draft = messageEditDraft(
    "ignored",
    rich(
      section(
        { type: "text", text: "Hi ", style: { bold: true } },
        { type: "user", user_id: "U1" },
        { type: "link", url: "https://example.com", text: "ex" },
      ),
    ),
    { U1: "Olena" },
  );
  assert.equal(
    html(draft),
    '<div><b>Hi </b><span contenteditable="false" data-user-id="U1">@Olena</span><a href="https://example.com">ex</a></div>',
  );
});

test("lists are rebuilt as real lists", () => {
  const list = { type: "rich_text_list", style: "bullet", elements: [
    section({ type: "text", text: "a" }), section({ type: "text", text: "b" }),
  ] };
  assert.equal(html(messageEditDraft("x", rich(list))), "<ul><li><div>a</div></li><li><div>b</div></li></ul>");
});

test("unsafe links lose their href and unsupported structures are refused", () => {
  const unsafe = messageEditDraft("x", rich(section({ type: "link", url: "javascript:alert(1)", text: "bad" })));
  assert.equal(html(unsafe), "<div>bad</div>");
  assert.equal(messageEditDraft("x", [{ type: "section" }]), null);
  assert.equal(messageEditDraft("x", {}), null);
});

test("edit and delete shortcuts ignore modifiers and composition", () => {
  const key = (code: string, extra: object = {}) =>
    messageActionKey({ code, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, isComposing: false, ...extra });
  assert.equal(key("KeyE"), "edit");
  assert.equal(key("Delete"), "delete");
  assert.equal(key("Backspace"), "delete");
  assert.equal(key("KeyR"), undefined);
  assert.equal(key("KeyE", { metaKey: true }), undefined);
  assert.equal(key("KeyE", { shiftKey: true }), undefined);
  assert.equal(key("KeyE", { isComposing: true }), undefined);
});
