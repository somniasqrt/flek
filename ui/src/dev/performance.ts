import { frameStatistics } from "../models/performance-budget";

/** Opt-in local measurements. Reports contain counts/timings, never message content. */
export const performanceEnabled =
  (import.meta.env.DEV || import.meta.env.VITE_PERF === "1") &&
  new URLSearchParams(location.search).get("perf") === "1";

type ScrollOptions = { durationMs?: number; pixelsPerSecond?: number };
type ScrollReport = {
  elapsedMs: number;
  frames: number;
  rafFps: number;
  p50FrameMs: number;
  p95FrameMs: number;
  maxFrameMs: number;
  over25ms: number;
  estimatedMissed60HzFrames: number;
  loadedMessages: number;
  maxMountedRows: number;
  distancePx: number;
};
type Probe = {
  startup?: { navigationMs: number; entryMs: number; conversations: number };
  scroll: (options?: ScrollOptions) => Promise<ScrollReport>;
};

declare global {
  interface Window { __flekPerf?: Probe }
}

let timeline: { element: HTMLElement; count: () => number } | undefined;
let running = false;
let sidebarPending = false;

if (performanceEnabled) {
  performance.mark("flek:ui-entry");
  window.__flekPerf = { scroll: scrollProbe };
}

/** Register only the active timeline; cleanup cannot detach a newer instance. */
export function attachTimelineProbe(element: HTMLElement, count: () => number): () => void {
  if (!performanceEnabled) return () => {};
  const attached = { element, count };
  timeline = attached;
  return () => { if (timeline === attached) timeline = undefined; };
}

/** Two rAFs allow one rendering opportunity after Solid commits the cached list.
 * This is a paint proxy: actual compositor presentation needs a native trace. */
export function markSidebarPaint(element: HTMLElement, count: () => number) {
  if (!performanceEnabled || sidebarPending || window.__flekPerf?.startup || !count()) return;
  sidebarPending = true;
  requestAnimationFrame(() => requestAnimationFrame(() => {
    sidebarPending = false;
    if (!element.isConnected || document.visibilityState !== "visible" || !count()) return;
    performance.mark("flek:sidebar-paint");
    const navigation = performance.measure("flek:navigation-to-sidebar-paint", {
      start: 0, end: "flek:sidebar-paint",
    });
    const entry = performance.measure("flek:entry-to-sidebar-paint", "flek:ui-entry", "flek:sidebar-paint");
    window.__flekPerf!.startup = {
      navigationMs: navigation.duration, entryMs: entry.duration, conversations: count(),
    };
  }));
}

async function scrollProbe(options: ScrollOptions = {}): Promise<ScrollReport> {
  if (running) throw new Error("A scroll probe is already running");
  if (!timeline?.count()) throw new Error("Open a populated timeline first");
  const duration = options.durationMs ?? 10_000;
  const speed = options.pixelsPerSecond ?? 2400;
  if (!Number.isFinite(duration) || duration < 1000 || duration > 60_000 ||
      !Number.isFinite(speed) || speed <= 0 || speed > 1_000_000)
    throw new Error("Expected durationMs 1000–60000 and pixelsPerSecond 1–1000000");
  const attached = timeline;
  const element = attached.element;
  if (element.scrollHeight <= element.clientHeight) throw new Error("Timeline is not scrollable");
  running = true;
  const gaps: number[] = [];
  let maxMountedRows = 0;
  let distancePx = 0;
  let direction = -1;
  let started = 0;
  let previous = 0;
  let lastTop = element.scrollTop;
  performance.clearMarks("flek:scroll-start");
  performance.clearMarks("flek:scroll-end");
  performance.clearMeasures("flek:scroll");
  try {
    return await new Promise<ScrollReport>((resolve, reject) => {
      // rAF can stop altogether in a hidden tab; always release the active probe.
      const watchdog = window.setTimeout(() => finish(new Error("Scroll probe timed out")), duration + 3000);
      let frame = 0;
      const finish = (error?: Error, report?: ScrollReport) => {
        clearTimeout(watchdog);
        cancelAnimationFrame(frame);
        document.removeEventListener("visibilitychange", hidden);
        if (error) reject(error);
        else resolve(report!);
      };
      const hidden = () => { if (document.visibilityState !== "visible") finish(new Error("Keep the measured tab visible")); };
      document.addEventListener("visibilitychange", hidden);
      const tick = (now: number) => {
        if (timeline !== attached || !element.isConnected) return finish(new Error("Timeline changed during probe"));
        if (document.visibilityState !== "visible") return finish(new Error("Keep the measured tab visible"));
        if (!started) {
          started = previous = now;
          performance.mark("flek:scroll-start");
        } else {
          const gap = now - previous;
          gaps.push(gap);
          distancePx += Math.abs(element.scrollTop - lastTop);
          lastTop = element.scrollTop;
          previous = now;
        }
        maxMountedRows = Math.max(maxMountedRows, element.querySelectorAll("[data-virtual-key]").length);
        const elapsed = now - started;
        if (elapsed >= duration) {
          performance.mark("flek:scroll-end");
          performance.measure("flek:scroll", "flek:scroll-start", "flek:scroll-end");
          return finish(undefined, {
            elapsedMs: elapsed, ...frameStatistics(gaps, elapsed),
            loadedMessages: attached.count(), maxMountedRows, distancePx,
          });
        }
        const max = element.scrollHeight - element.clientHeight;
        if (element.scrollTop <= 1) direction = 1;
        else if (element.scrollTop >= max - 1) direction = -1;
        element.scrollTop = Math.max(0, Math.min(max, element.scrollTop + direction * speed * (gaps.at(-1) ?? 1000 / 60) / 1000));
        frame = requestAnimationFrame(tick);
      };
      frame = requestAnimationFrame(tick);
    });
  } finally {
    running = false;
  }
}
