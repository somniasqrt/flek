import test from "node:test";
import assert from "node:assert/strict";
import { canOpen, fileName, isRasterImage, unavailable } from "./file-presentation.ts";

test("real name wins over a misleading sender title and bidi controls", () => {
  const file = { name: "x.terminal", title: "Розклад.pdf" };
  assert.equal(fileName(file, "file"), "x.terminal");
  assert.equal(fileName({ name: "Розклад\u202efdp.command" }, "file"), "Розкладfdp.command");
  assert.equal(fileName({ name: null }, "file"), "file");
});
test("UI uses core launch decision and fails closed for missing decisions", () => {
  assert.equal(canOpen({ name: "x.pdf", can_open: true }), true);
  assert.equal(canOpen({ name: "x.terminal", can_open: false }), false);
  assert.equal(canOpen({ name: "x.pdf" }), false);
});

test("only raster formats can be inline images", () => {
  for (const mime of ["image/png", "image/jpeg", "image/webp", "IMAGE/JPEG; charset=binary", "image/x-ms-bmp"])
    assert.equal(isRasterImage(mime), true);
  for (const mime of ["image/svg+xml", "text/html", "text/xml", null]) assert.equal(isRasterImage(mime), false);
});

test("inline video requires the cache and explicit core launch approval", async () => {
  const { isInlineVideo } = await import("./file-presentation.ts");
  const f = { mimetype: "video/mp4", local_path: "/synthetic/video.mp4", can_open: true, hidden_by_limit: false };
  assert.equal(isInlineVideo(f), true);
  assert.equal(isInlineVideo({ ...f, mimetype: "Video/MP4; codecs=avc1" }), true);
  for (const patch of [{ local_path: null }, { can_open: false }, { can_open: undefined }, { hidden_by_limit: true }, { mimetype: "video/html" }])
    assert.equal(isInlineVideo({ ...f, ...patch }), false);
});

test("deleted files stay usable only through a saved copy", () => {
  assert.equal(unavailable({ hidden_by_limit: false, deleted: true, local_path: null }), true);
  assert.equal(unavailable({ hidden_by_limit: false, deleted: true, local_path: "/cache/F1/a.pdf" }), false);
  assert.equal(unavailable({ hidden_by_limit: true, local_path: "/cache/F1/a.pdf" }), true);
  assert.equal(unavailable({ hidden_by_limit: false, local_path: null }), false, "not downloaded yet is still available");
});
