/** Nearest-rank percentiles; the input array remains available in sample order. */
export function percentile(values: readonly number[], fraction: number): number {
  if (!values.length) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1))];
}

/** rAF scheduling statistics relative to a 60 Hz target, not compositor FPS. */
export function frameStatistics(gaps: readonly number[], elapsedMs: number) {
  return {
    frames: gaps.length,
    rafFps: elapsedMs > 0 ? gaps.length * 1000 / elapsedMs : 0,
    p50FrameMs: percentile(gaps, 0.5),
    p95FrameMs: percentile(gaps, 0.95),
    maxFrameMs: gaps.reduce((max, gap) => Math.max(max, gap), 0),
    over25ms: gaps.filter((gap) => gap > 25).length,
    estimatedMissed60HzFrames: gaps.reduce((sum, gap) => sum + Math.max(0, Math.round(gap / (1000 / 60)) - 1), 0),
  };
}
