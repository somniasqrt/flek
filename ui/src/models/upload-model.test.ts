import assert from "node:assert/strict";
import test from "node:test";
import {
  MAX_UPLOAD_BYTES,
  uploadDisplayName,
  uploadFileError,
  uploadPercent,
} from "./upload-model.ts";

test("uploadFileError enforces the core staging limits", () => {
  assert.equal(uploadFileError({ name: "a.txt", size: 1 }), null);
  assert.equal(uploadFileError({ name: "a.txt", size: MAX_UPLOAD_BYTES }), null);
  assert.equal(uploadFileError({ name: "a.txt", size: 0 }), "upload_empty");
  assert.equal(uploadFileError({ name: "a.txt", size: -1 }), "upload_empty");
  assert.equal(uploadFileError({ name: "a.txt", size: Number.NaN }), "upload_empty");
  assert.equal(uploadFileError({ name: "a.txt", size: MAX_UPLOAD_BYTES + 1 }), "upload_too_large");
});

test("uploadFileError rejects names core would refuse", () => {
  for (const name of ["", "a/b", "a\\b", "tab\there", "nul\u0000", "x".repeat(256)]) {
    assert.equal(uploadFileError({ name, size: 5 }), "upload_bad_name", JSON.stringify(name));
  }
  assert.equal(uploadFileError({ name: "x".repeat(255), size: 5 }), null);
  // The limit is in UTF-8 bytes, like core's `filename.len()`.
  assert.equal(uploadFileError({ name: "я".repeat(128), size: 5 }), "upload_bad_name");
  assert.equal(uploadFileError({ name: "звіт 2026.pdf", size: 5 }), null);
});

test("uploadPercent clamps and tolerates missing progress", () => {
  assert.equal(uploadPercent(undefined), 0);
  assert.equal(uploadPercent({ stage: "uploading", sent: 0, total: 0 }), 0);
  assert.equal(uploadPercent({ stage: "uploading", sent: 50, total: 200 }), 25);
  assert.equal(uploadPercent({ stage: "uploading", sent: 999, total: 200 }), 100);
  assert.equal(uploadPercent({ stage: "uploading", sent: -5, total: 200 }), 0);
});

test("uploadDisplayName strips bidirectional controls only", () => {
  assert.equal(uploadDisplayName("fi‮txt.exe"), "fitxt.exe");
  assert.equal(uploadDisplayName("⁦x⁩.pdf‎"), "x.pdf");
  assert.equal(uploadDisplayName("звіт.pdf"), "звіт.pdf");
});
