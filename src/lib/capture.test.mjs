import test from "node:test";
import assert from "node:assert/strict";
import { captureUrls, captureFilename } from "./capture.ts";

test("batch capture normalizes links, strips fragments and suppresses duplicates", () => {
  assert.deepEqual(
    captureUrls(
      "\uFEFFhttps://EXAMPLE.invalid/a#one\r\n# comment\nhttps://example.invalid/a#two\n\nhttps://example.invalid/b",
    ),
    ["https://example.invalid/a", "https://example.invalid/b"],
  );
});
test("capture refuses credentials and unsafe decoded names", () => {
  assert.throws(() => captureUrls("https://user:password@example.invalid/a"));
  assert.throws(() => captureUrls("file:///a"));
  for (const path of ["", "a%2fb", "CON.txt", "bad%00.txt", "end."])
    assert.throws(() => captureFilename(`https://example.invalid/${path}`));
  assert.equal(captureFilename("https://example.invalid/a%20b.zip"), "a b.zip");
});
