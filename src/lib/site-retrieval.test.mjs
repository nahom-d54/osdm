import test from "node:test";
import assert from "node:assert/strict";
import { scopeFor, siteLimits } from "./site-retrieval.ts";

test("default scope stays at the explicit seed directory", () => {
  assert.equal(scopeFor("https://docs.example.test/manual/index.html?token=synthetic"), "/manual/");
  assert.equal(scopeFor("https://docs.example.test/manual/"), "/manual/");
});
test("site budgets cannot become unknown, negative, fractional or unbounded", () => {
  const valid = { pages: "20", files: "100", mebibytes: "64", depth: "3" };
  assert.deepEqual(siteLimits(valid), { max_pages: 20, max_files: 100, max_bytes: 67108864, max_depth: 3 });
  for (const [key, value] of [["pages", "0"], ["files", "10001"], ["mebibytes", "Infinity"], ["mebibytes", "10241"], ["depth", "1.5"], ["pages", "-1"], ["depth", "17"]]) assert.throws(() => siteLimits({ ...valid, [key]: value }));
});
