import { strict as assert } from "node:assert";
import { test } from "node:test";
import { externalLink } from "./external-link.ts";
import { readFileSync, readdirSync } from "node:fs";

test("external links have no native destination and activate through the opener", () => {
  const opened: string[] = [];
  const props = externalLink("https://example.test/path", (url) => opened.push(url));
  assert.equal("href" in props, false);
  assert.equal(props.role, "link");
  assert.equal(props.tabIndex, 0);
  assert.equal(props.draggable, false);
  assert.equal(props.title, "https://example.test/path");
  let prevented = 0;
  const event = { preventDefault: () => prevented++ };
  props.onClick(event);
  props.onKeyDown({ ...event, key: "Enter", repeat: false } as KeyboardEvent);
  props.onKeyDown({ ...event, key: " ", repeat: false } as KeyboardEvent);
  props.onKeyDown({ ...event, key: "Enter", repeat: true } as KeyboardEvent);
  props.onAuxClick({ ...event, button: 1 } as MouseEvent);
  props.onAuxClick({ ...event, button: 2 } as MouseEvent);
  props.onContextMenu(event);
  props.onDragStart(event);
  assert.deepEqual(opened, Array(3).fill(props.title));
  assert.equal(prevented, 6);
  const absent = externalLink(null, (url) => opened.push(url));
  absent.onClick(event);
  assert.equal(absent.tabIndex, -1);
  assert.equal(absent["aria-disabled"], true);
  assert.equal(opened.length, 3);
});

test("message and profile links use the safe interaction props without href", () => {
  for (const path of ["components/Rich.tsx", "components/Unfurls.tsx", "views/ProfilePanel.tsx"]) {
    const source = readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
    assert.doesNotMatch(source, /\bhref\s*=/);
    assert.match(source, /externalLink\(/);
  }
});

test("UI has no document embeds or submitting form actions; PDF uses IPC canvas", () => {
  const root = new URL("../", import.meta.url);
  for (const path of readdirSync(root, { recursive: true }).filter((path) => path.endsWith(".tsx"))) {
    const source = readFileSync(new URL(path, root), "utf8");
    assert.doesNotMatch(source, /<(?:iframe|object|base)\b|<form\b[^>]*\baction\s*=/);
  }
  const viewer = readFileSync(new URL("../components/Viewer.tsx", import.meta.url), "utf8");
  assert.match(viewer, /api\.pdfData/);
  assert.match(viewer, /<canvas/);
});
