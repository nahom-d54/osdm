import { test } from "node:test";
import assert from "node:assert/strict";
import { runBulk } from "./bulk.ts";

test("bulk operations finish all entries and report partial failure without source secrets", async () => {
  const seen = [];
  await assert.rejects(
    runBulk(["a", "b", "c"], async (id) => {
      seen.push(id);
      if (id === "b") throw new Error("private source");
    }),
    {
      message:
        "1 of 3 operations failed. Refresh the list and retry the remaining entries.",
    },
  );
  assert.deepEqual(seen, ["a", "b", "c"]);
  await runBulk([], async () => assert.fail("empty batch"));
});

test("synchronous exceptions cannot prevent other selected operations", async () => {
  const seen = [];
  await assert.rejects(
    runBulk(["a", "b"], (id) => {
      seen.push(id);
      if (id === "a") throw new Error("failure");
      return Promise.resolve();
    }),
  );
  assert.deepEqual(seen, ["a", "b"]);
});
