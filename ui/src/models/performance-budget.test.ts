import assert from "node:assert/strict";
import { test } from "node:test";
import { frameStatistics, percentile } from "./performance-budget.ts";

test("percentile uses nearest rank and leaves its input untouched", () => {
  const values = [30, 10, 20, 40, 50];
  assert.equal(percentile(values, 0.5), 30);
  assert.equal(percentile(values, 0.95), 50);
  assert.equal(percentile(values, 0), 10);
  assert.deepEqual(values, [30, 10, 20, 40, 50]);
  assert.equal(percentile([], 0.5), 0);
  assert.equal(percentile([7], 0.95), 7);
});

test("frame statistics report a steady 60 Hz run as 60 fps without misses", () => {
  const gaps = Array.from({ length: 60 }, () => 1000 / 60);
  const s = frameStatistics(gaps, 1000);
  assert.equal(s.frames, 60);
  assert.equal(Math.round(s.rafFps), 60);
  assert.equal(s.over25ms, 0);
  assert.equal(s.estimatedMissed60HzFrames, 0);
  assert.ok(s.p95FrameMs < 17);
});

test("a long frame is counted as the 60 Hz frames it displaced", () => {
  const s = frameStatistics([16.7, 16.7, 50, 16.7], 100.1);
  assert.equal(s.over25ms, 1);
  assert.equal(s.maxFrameMs, 50);
  assert.equal(s.estimatedMissed60HzFrames, 2); // 50 ms ≈ 3 frames, so 2 missed
});

test("no elapsed time or frames yields zeros, not NaN", () => {
  const s = frameStatistics([], 0);
  assert.equal(s.rafFps, 0);
  assert.equal(s.p50FrameMs, 0);
  assert.equal(s.maxFrameMs, 0);
});
