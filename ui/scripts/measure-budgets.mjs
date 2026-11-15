// Local synthetic measurement runner, never the Tauri app or a signed-in browser.
import { createRequire } from "node:module";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { build, preview } from "vite";

const require = createRequire(import.meta.url);
const playwright = require(process.env.PLAYWRIGHT_MODULE || "playwright");
const engine = process.env.FLEK_BROWSER || "webkit";
if (!["webkit", "chromium", "firefox"].includes(engine)) throw new Error("Unknown FLEK_BROWSER");
const samples = Number(process.env.FLEK_UI_SAMPLES ?? 5);
if (!Number.isInteger(samples) || samples < 1 || samples > 100) throw new Error("FLEK_UI_SAMPLES must be 1–100");
const durationMs = Number(process.env.FLEK_SCROLL_MS ?? 10_000);
const root = fileURLToPath(new URL("../", import.meta.url));
const temp = await mkdtemp(path.join(tmpdir(), "flek-budget-ui-"));
let server;
let browser;
try {
  process.env.VITE_PERF = "1";
  const config = {
    root,
    configFile: path.join(root, "vite.config.ts"),
    configLoader: "runner",
    cacheDir: path.join(temp, "cache"),
    logLevel: "warn",
    build: {
      outDir: path.join(temp, "dist"),
      rolldownOptions: { input: { preview: path.join(root, "preview.html") } },
    },
  };
  await build(config);
  server = await preview({ ...config, preview: { host: "127.0.0.1", port: 5178, strictPort: false, open: false } });
  const origin = server.resolvedUrls.local[0];
  browser = await playwright[engine].launch({ headless: process.env.FLEK_HEADED !== "1" });
  const startups = [];
  const scrolls = [];
  for (let sample = 0; sample < samples; sample++) {
    // A fresh context gives each navigation an empty browser cache/localStorage.
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
    try {
      await context.route("**/*", (route) => {
        const url = new URL(route.request().url());
        return url.origin === new URL(origin).origin ? route.continue() : route.abort();
      });
      await context.addInitScript(() => {
        localStorage.setItem("flek.lastConversation", "CBIG");
        localStorage.setItem("flek.rail.T1.U3", "home");
      });
      const page = await context.newPage();
      await page.goto(`${origin}preview.html?perf=1&stress=1`, { waitUntil: "load" });
      await page.waitForFunction(() => !!window.__flekPerf?.startup, null, { timeout: 30_000 });
      startups.push(await page.evaluate(() => ({
        ...window.__flekPerf.startup,
        firstContentfulPaintMs: performance.getEntriesByName("first-contentful-paint")[0]?.startTime ?? null,
      })));
      await page.waitForFunction(() => !!document.querySelector('.conv[data-conversation-id="CBIG"][aria-current="page"]') && !!document.querySelector(".virtual-row"));
      // Allow initial ResizeObserver/scroll restoration to settle before sampling.
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(1000);
      const scroll = await page.evaluate((durationMs) => window.__flekPerf.scroll({ durationMs }), durationMs);
      if (scroll.loadedMessages !== 10_000 || scroll.distancePx <= 0 || !scroll.maxMountedRows)
        throw new Error("Invalid scroll measurement: expected 10k loaded messages and a moving viewport");
      scrolls.push(scroll);
    } finally {
      await context.close();
    }
  }
  const p = (values, fraction) => [...values].sort((a, b) => a - b)[Math.max(0, Math.ceil(values.length * fraction) - 1)];
  console.log(JSON.stringify({
    browser: engine, browserVersion: browser.version(), headless: process.env.FLEK_HEADED !== "1",
    platform: process.platform, arch: process.arch, viewport: { width: 1440, height: 900 },
    build: "production preview with VITE_PERF=1; fresh browser context per sample",
    note: "Synthetic mock IPC; rAF is a scheduling proxy, not compositor FPS or native launch time",
    samples, startups, scrolls,
    summary: {
      navigationP50Ms: p(startups.map((s) => s.navigationMs), 0.5),
      navigationP95Ms: p(startups.map((s) => s.navigationMs), 0.95),
      entryP50Ms: p(startups.map((s) => s.entryMs), 0.5),
      scrollFpsP50: p(scrolls.map((s) => s.rafFps), 0.5),
      worstP95FrameMs: Math.max(...scrolls.map((s) => s.p95FrameMs)),
      maxMountedRows: Math.max(...scrolls.map((s) => s.maxMountedRows)),
    },
  }, null, 2));
} finally {
  await browser?.close();
  await new Promise((resolve, reject) => server ? server.httpServer.close((error) => error ? reject(error) : resolve()) : resolve());
  await rm(temp, { recursive: true, force: true });
}
