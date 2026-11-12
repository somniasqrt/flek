import test from "node:test";
import assert from "node:assert/strict";
import { VirtualLayout, localDateBounds } from "./virtualizer.ts";

test("10k rows have a bounded visible window and exact spacer sums", () => {
  const v = new VirtualLayout();
  v.reset(
    Array.from({ length: 10_000 }, (_, i) => String(i)),
    () => 80,
  );
  assert.equal(v.total, 800_000);
  for (const top of [0, 1, 400_000, 799_999]) {
    const r = v.range(top, 600);
    assert.ok(r.end - r.start < 25);
    assert.equal(r.before + (r.end - r.start) * 80 + r.after, v.total);
    assert.ok(v.offset(r.start) <= top);
  }
  assert.equal(v.indexAt(80), 1);
  assert.equal(v.indexAt(-30), 0);
});
test("height changes and prepends preserve the keyed reading anchor", () => {
  const v = new VirtualLayout();
  v.reset(["a", "b", "c"], () => 100);
  const anchor = v.anchor(125);
  v.measure("a", 240);
  assert.equal(v.restore(anchor), 265);
  v.reset(["older", "a", "b", "c"], () => 100);
  assert.equal(v.restore(anchor), 365);
  assert.equal(v.total, 540);
  v.measure("b", 250);
  assert.equal(v.restore(anchor), 365);
  assert.equal(v.measure("b", NaN), false);
  assert.equal(v.measure("b", 0), false);
  v.reset(["c"]);
  assert.equal(v.restore(anchor), undefined);
  v.clear();
  assert.equal(v.total, 0);
  assert.deepEqual(v.range(0, 500), { start: 0, end: 0, before: 0, after: 0 });
});
test("mixed height seeks agree with linear sums after repeated remeasurements", () => {
  const v = new VirtualLayout();
  const keys = Array.from({ length: 100 }, (_, i) => String(i));
  v.reset(keys, () => 50);
  const heights = keys.map((key, i) => {
    const h = 20 + ((i * 37) % 400);
    v.measure(key, h);
    return h;
  });
  let sum = 0;
  for (let i = 0; i < heights.length; i++) {
    assert.equal(v.offset(i), sum);
    assert.equal(v.indexAt(sum + heights[i] - 1), i);
    sum += heights[i];
  }
  assert.equal(v.total, sum);
});
test("date picker bounds are local midnight to next calendar day and reject invalid dates", () => {
  const b = localDateBounds("2026-11-12")!;
  assert.equal(new Date(Number(b.start) * 1000).getHours(), 0);
  assert.equal(new Date(Number(b.end) * 1000).getDate(), 13);
  for (const value of ["", "2026-02-30", "no", "2026-13-01"])
    assert.equal(localDateBounds(value), null);
});
