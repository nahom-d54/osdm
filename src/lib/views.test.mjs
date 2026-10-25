import { test } from "node:test";
import assert from "node:assert/strict";
import { matchesQuery, matchesView } from "./views.ts";

test("unfinished grouping and description search use durable engine data", () => {
  for (const state of [
    "Queued",
    "Downloading",
    "Pausing",
    "Paused",
    "Canceling",
    "Canceled",
    "Failed",
  ]) {
    assert.equal(matchesView("unfinished", { state }), true);
  }
  assert.equal(matchesView("unfinished", { state: "Completed" }), false);
  const download = {
    url: "https://example.test/file",
    destination: "C:\\Downloads\\file.bin",
    properties: { description: "Release archive" },
  };
  assert.equal(matchesQuery(download, "RELEASE"), true);
  assert.equal(matchesQuery(download, "absent"), false);
});
