import assert from "node:assert/strict";
import { test } from "node:test";
import { clearDrafts, draftKey, escapeText, newClientMsgId, nowTs, outgoingText } from "./compose.ts";

test("escapes what Slack reserves, nothing else", () => {
  assert.equal(escapeText("a < b && c > d *bold* <@U1>"), "a &lt; b &amp;&amp; c &gt; d *bold* &lt;@U1&gt;");
  assert.equal(escapeText("привіт :wave: _x_ `y`"), "привіт :wave: _x_ `y`");
});

test("outgoing text is trimmed, inner newlines kept", () => {
  assert.equal(outgoingText("  \n hi\nthere \n\n"), "hi\nthere");
  assert.equal(outgoingText(" \n\t "), "");
});

test("drafts are per conversation and per thread", () => {
  assert.notEqual(draftKey("C1"), draftKey("C2"));
  assert.notEqual(draftKey("C1"), draftKey("C1", "1.0"));
  assert.equal(draftKey("C1", null), draftKey("C1"));
});

test("client ids are UUID-shaped and fresh", () => {
  const a = newClientMsgId();
  assert.match(a, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.notEqual(a, newClientMsgId());
});

test("ts for now", () => {
  assert.equal(nowTs(1762600000123), "1762600000.123000");
});

test("sign-out removes every draft and nothing else", () => {
  const data = new Map<string, string>([
    [draftKey("C1"), "hello"],
    ["flek.layout", "{}"],
    [draftKey("C1", "1.000000"), '{"v":1,"html":"<b>x</b>"}'],
    ["flek.emoji.recent", "[]"],
    [draftKey("D2"), "secret plan"],
  ]);
  const storage = {
    get length() {
      return data.size;
    },
    key: (i: number) => [...data.keys()][i] ?? null,
    removeItem: (k: string) => void data.delete(k),
  } as unknown as Storage;
  clearDrafts(storage);
  assert.deepEqual([...data.keys()].sort(), ["flek.emoji.recent", "flek.layout"]);
});

test("clearing drafts survives unavailable storage", () => {
  const broken = {
    get length(): number {
      throw new Error("denied");
    },
  } as unknown as Storage;
  assert.doesNotThrow(() => clearDrafts(broken));
});
