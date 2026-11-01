import test from "node:test";
import assert from "node:assert/strict";
import { scopeFor, siteLimits, utcSiteSchedule } from "./site-retrieval.ts";

test("default scope stays at the explicit seed directory", () => {
  assert.equal(scopeFor("https://docs.example.test/manual/index.html?token=synthetic"), "/manual/");
  assert.equal(scopeFor("https://docs.example.test/manual/"), "/manual/");
});
test("site budgets cannot become unknown, negative, fractional or unbounded", () => {
  const valid = { pages: "20", files: "100", mebibytes: "64", depth: "3" };
  assert.deepEqual(siteLimits(valid), { max_pages: 20, max_files: 100, max_bytes: 67108864, max_depth: 3 });
  for (const [key, value] of [["pages", "0"], ["files", "10001"], ["mebibytes", "Infinity"], ["mebibytes", "10241"], ["depth", "1.5"], ["pages", "-1"], ["depth", "17"]]) assert.throws(() => siteLimits({ ...valid, [key]: value }));
});

test("site schedule is explicit UTC, bounded, and not normalized from invalid dates", () => {
  assert.deepEqual(utcSiteSchedule("", "60"), { next_run: null, interval_seconds: null });
  assert.deepEqual(utcSiteSchedule("2100-01-01T04:00", "60"), { next_run: Date.parse("2100-01-01T04:00:00Z") / 1000, interval_seconds: 3600 });
  for (const value of ["2100-02-30T04:00", "2026-01-01T24:99", "not-a-date"]) assert.throws(() => utcSiteSchedule(value, "60"));
  for (const repeat of ["0", "-1", "1.5", "525601", "Infinity"]) assert.throws(() => utcSiteSchedule("2100-01-01T04:00", repeat));
});
