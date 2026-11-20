# Performance budgets (M6.6)

| Budget | Step | Where it is measured |
| --- | --- | --- |
| Cold start < 300 ms to first painted channel list from cache | M6.6.1 | core: cache open + start-up queries; UI: sidebar paint mark |
| Scrolling a 10k-message timeline at 60 fps | M6.6.2 | UI scroll probe (`window.__flekPerf.scroll`) |
| Local search < 50 ms | M6.6.3 | core: `search_*` rows of the budget benchmark |

Everything runs on synthetic data. No Slack, Keychain, real DB or app process is involved.

## Core (SQLite) benchmark

```sh
cargo bench -p flek-core --bench budgets          # release profile, 30 samples
FLEK_BENCH_SAMPLES=100 cargo bench -p flek-core --bench budgets
FLEK_BENCH_KEEP_DB=/path/synthetic.db cargo bench -p flek-core --bench budgets   # keep the ~170 MB DB for EXPLAIN QUERY PLAN
```

- Generates a deterministic 100k-message file-backed DB (650 users, 100 conversations, 768 bytes of
  `raw_json` padding per message, seed 42; about 30-45 s and ~170 MB of temp disk) via
  `crates/core/benches/support/mod.rs`, then prints one JSON report: first/min/p50/p95/max per
  measurement, the budget and whether **p95** is inside it.
- Measured: connection open, the cached start-up reads (identity, sidebar, DMs, activity badge, icon,
  people, avatars, emoji), each separately, timeline pages/windows, and FTS search for six queries
  in both sort orders (parse + snippets included).
- Always use the release profile (`cargo bench` does). A debug run is labelled and its budgets are
  meaningless. The OS file cache is not flushed, so "cold" means a freshly opened connection only;
  true first-launch latency after a reboot is not covered.
- Other agents or builds running at the same time skew the numbers (p95 more than p50). Repeat on an
  idle machine before drawing conclusions.
- Search regression and before/after timing: `cargo test -p flek-core --test search_regression`
  (equivalence with the original single-query implementation on a synthetic DB), and
  `cargo test --release -p flek-core --test search_regression -- --ignored --nocapture` for the
  original-vs-current timing on the 100k fixture. `cargo test --release -p flek-core --test search -- --ignored --nocapture`
  prints the older per-query timings.

## UI measurements

Instrumentation lives in `ui/src/dev/performance.ts` and is inert unless the page URL has `?perf=1`
**and** the build is a dev server or has `VITE_PERF=1`. Reports contain counts and timings only.

- `window.__flekPerf.startup`: set once the cached sidebar has painted (two animation frames after
  Solid commits the list). `navigationMs` counts from navigation start, `entryMs` from the first
  statement of the UI entry module. Marks/measures: `flek:ui-entry`, `flek:sidebar-paint`,
  `flek:navigation-to-sidebar-paint`, `flek:entry-to-sidebar-paint`.
- `await window.__flekPerf.scroll({ durationMs: 10000, pixelsPerSecond: 2400 })`: scrolls the open
  timeline up and down for the duration and returns rAF frame statistics (fps, p50/p95/max frame
  gap, frames over 25 ms, estimated missed 60 Hz frames), loaded message count, the maximum number
  of mounted `[data-virtual-key]` rows and the distance scrolled. The tab must stay visible.

By hand: `npm run dev --workspace ui`, open
`http://localhost:5173/preview.html?perf=1&stress=1`, select **timeline-10000**, then run the call
above in the console.

Automated (Playwright WebKit, same engine as WKWebView on macOS):

```sh
cd ui
PLAYWRIGHT_MODULE=/abs/path/to/node_modules/playwright \
FLEK_UI_SAMPLES=7 FLEK_SCROLL_MS=8000 node scripts/measure-budgets.mjs
```

It builds the preview with `VITE_PERF=1`, serves it locally (external requests are blocked), and uses
a fresh browser context per sample. Optional: `FLEK_BROWSER=chromium|firefox|webkit`, `FLEK_HEADED=1`.
The Playwright version must match the installed browsers in `~/Library/Caches/ms-playwright`.

Caveats: IPC is mocked, so startup excludes real SQLite/Tauri latency (add the core numbers); rAF
gaps are a scheduling proxy, not compositor frames; the first sample in a run is typically much
slower (cold WebKit process), so compare p50 and look at the individual samples.

## Results (2026-11-15, Apple silicon, release, machine shared with other builds)

Core, 100k messages / 167 MB DB, p50 / p95 in ms:

| Measurement | p50 | p95 | Budget |
| --- | --- | --- | --- |
| connection open | 113-159 | 133-270 | - |
| open + all cached start-up reads | 212-604 | 232-1004 | 300 |
| cached start-up reads on a warm connection | 99-169 | 333-391 | 300 |
| of which `rail::activity` (badge) | 85 | 102-160 | - |
| sidebar / people / DMs / identity | < 0.5 | < 0.5 | - |
| timeline page / window (60-120 rows) | 0.7-2.1 | 0.9-2.5 | - |
| search, rare word (`needle`) | 0.3-0.5 | 0.5-0.8 | 50 |
| search, frequent word / prefix / filters | 60-280 | 80-690 | 50 |

(The two ranges are two runs, one on a quieter machine.) Start-up fits the budget on a quiet machine
(open + reads p95 about 230 ms) but not under load; the activity badge query is ~85% of the read
time. UI (preview, mocked IPC, 10k-message timeline, 7 samples): sidebar painted at 238 ms p50 from
navigation start (192 ms from entry, 177-695 ms range), scroll 60.0 fps rAF with p95 frame gap
17-18 ms and at most 32 mounted rows, so the timeline is already virtualized and meets M6.6.2 in
this proxy. Frequent-word search misses the 50 ms budget.

## Start-up after M6.6.1 (2026-11-20, quiet machine, 30 samples)

Two changes took start-up from 212-604 ms p50 to about 1 ms:

- `Store::open` ran `pragma_foreign_key_check` on every open. It walks every foreign key in the
  database (~55 ms of the ~56 ms open on the 100k fixture). It now runs only when a migration was
  applied in that open, which is the only time a violation can appear. Open: 0.6 ms p50.
- The Activity badge (`rail::activity`, ~30 ms) no longer blocks the first paint: the UI loads it
  separately after the sidebar data, and the benchmark's `cached_startup_reads` covers only what
  the sidebar waits for. The badge is still measured as `activity_badge`.

| Measurement | p50 | p95 | Budget |
| --- | --- | --- | --- |
| connection open | 0.64 | 0.78 | - |
| open + reads before first paint | 1.19 | 1.35 | 300 |
| activity badge (after paint) | 30.1 | 30.8 | - |

## Search: what was done and what remains

Done: newest-first free-text search selects the page of message ids first and renders `snippet()`
only for that page (`store/search.rs`, `page_first`). On the 100k fixture this cut the frequent-word
newest-first case from ~400-800 ms to ~100-250 ms with identical results, order and pagination
(regression test above). Relevance order and filter-only queries keep the original query: the
rewrite was slower for them.

Still over budget (frequent words, 40-50% of messages matching). The floor is reading the full
`messages` row for every FTS hit just to sort by `ts` or apply the `team`/`deleted` filters, ~90 ms
here because rows carry large `raw_json`. Plan, not implemented:

1. Add a narrow table `message_search(id PK, ts, conversation_id, team_id, deleted)` (or put
   `ts`/`conversation_id` into FTS5 as `UNINDEXED` columns), kept in sync by the existing FTS
   triggers, backfilled by a numbered migration. Sort/filter on it instead of touching `messages`.
2. Newest-first with a frequent term: walk `messages_activity (ts DESC, conversation_id)` and probe
   FTS per row with `rowid = ? AND MATCH`; that is ~40 ms for `grades` but takes seconds for rare
   words, so use it only with a bound (probe the newest N rows, fall back to the MATCH-first plan
   when the page does not fill), picking N from FTS5's `fts5vocab` document counts or a row cap.
3. Relevance: bm25 needs every hit. Options are a candidate cap (top ~2000 hits by rowid recency)
   with a "more results" fallback, or accepting >50 ms for frequent single-word queries and
   showing the first page progressively (page 1 from a bounded candidate set).
4. ~~Cache the activity badge count~~: superseded, the badge loads after the first paint (M6.6.1).
