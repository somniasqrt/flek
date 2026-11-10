import assert from "node:assert/strict";
import { beforeEach, describe, it } from "node:test";
import {
  DEFAULT_DETAIL,
  DEFAULT_SIDEBAR,
  MAX_DETAIL,
  MAX_SIDEBAR,
  MIN_DETAIL,
  MIN_SIDEBAR,
  migratePanelSize,
  panelSizes,
  readPanelSize,
} from "./panel-sizes.ts";

const store = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  value: {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  },
  configurable: true,
});

describe("panelSizes", () => {
  it("uses fractions of a normal window", () => {
    const s = panelSizes(1400, DEFAULT_SIDEBAR, DEFAULT_DETAIL, true);
    assert.equal(Math.round(s.sidebar), 280);
    assert.equal(Math.round(s.detail), 420);
    assert.equal(s.overlay, false);
  });

  it("applies pixel floors in a small window", () => {
    const s = panelSizes(722, DEFAULT_SIDEBAR, DEFAULT_DETAIL, false);
    assert.equal(s.sidebar, MIN_SIDEBAR);
    assert.equal(s.detail, 0);
  });

  it("overlays the detail panel in a narrow window", () => {
    const s = panelSizes(722, DEFAULT_SIDEBAR, DEFAULT_DETAIL, true);
    assert.equal(s.overlay, true);
    assert.ok(s.detail >= MIN_DETAIL && s.detail <= 722 - MIN_SIDEBAR);
  });

  it("caps panels in a huge window", () => {
    const s = panelSizes(3000, DEFAULT_SIDEBAR, DEFAULT_DETAIL, true);
    assert.equal(s.sidebar, MAX_SIDEBAR);
    assert.equal(s.detail, MAX_DETAIL);
  });

  it("keeps a minimum timeline when docked", () => {
    const s = panelSizes(1000, 0.4, 0.6, true);
    assert.ok(1000 - s.sidebar - s.detail >= 360);
  });
});

describe("migration", () => {
  beforeEach(() => store.clear());

  it("converts old pixel values to fractions and clamps", () => {
    assert.ok(Math.abs(migratePanelSize(300, 1500) - 0.2) < 1e-9);
    assert.equal(migratePanelSize(760, 700), 0.6);
    assert.equal(migratePanelSize(0.25, 1500), 0.25);
  });

  it("reads saved values with a fallback", () => {
    assert.equal(readPanelSize("k", 0.2, 1500), 0.2);
    store.set("k", "400");
    assert.ok(Math.abs(readPanelSize("k", 0.3, 1000) - 0.4) < 1e-9);
    store.set("k", "junk");
    assert.equal(readPanelSize("k", 0.3, 1000), 0.3);
  });
});
