import { test } from "node:test";
import assert from "node:assert/strict";
import { orderDownloads } from "./download-order.ts";

test("sorting is presentation-only, stable, and keeps unknown sizes last", () => {
  const downloads = [
    { id: "a", destination: "C:\\file10.zip", total_bytes: null },
    { id: "b", destination: "C:\\file2.zip", total_bytes: 20 },
    { id: "c", destination: "/file3.zip", total_bytes: 20 },
    { id: "d", destination: "/file1.zip", total_bytes: 10 },
  ];
  const ids = (sort, descending) =>
    orderDownloads(downloads, sort, descending).map((item) => item.id);
  assert.deepEqual(ids("file", false), ["d", "b", "c", "a"]);
  assert.deepEqual(ids("size", false), ["d", "b", "c", "a"]);
  assert.deepEqual(ids("size", true), ["b", "c", "d", "a"]);
  assert.deepEqual(ids("queue", false), ["a", "b", "c", "d"]);
});
