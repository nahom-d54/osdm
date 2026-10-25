import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_CATEGORIES, routeCategory } from "./categories.ts";

test("built-ins match case-insensitively and unknown files fall back to General", () => {
  assert.equal(
    routeCategory(DEFAULT_CATEGORIES, "https://example.test/file", "SONG.MP3")
      ?.id,
    "music",
  );
  assert.equal(
    routeCategory(
      DEFAULT_CATEGORIES,
      "https://example.test/file.zip",
      "file.unknown",
    ),
    undefined,
  );
  assert.equal(routeCategory(DEFAULT_CATEGORIES, "invalid", ".zip"), undefined);
});

test("exact original hostname wins over extensions without matching URL secrets or suffixes", () => {
  const domain = {
    id: "work",
    name: "Work",
    extensions: [],
    domains: ["example.test"],
    directory: "D:\\Work",
  };
  const categories = [...DEFAULT_CATEGORIES, domain];
  assert.equal(
    routeCategory(categories, "https://EXAMPLE.test./file", "music.mp3"),
    domain,
  );
  for (const source of [
    "https://notexample.test/file",
    "https://sub.example.test/file",
    "https://example.test@other.test/file",
    "https://other.test/?host=example.test",
  ]) {
    assert.equal(routeCategory(categories, source, "music.mp3")?.id, "music");
  }
});

test("ordered conflicts, compound extensions, and deletion have deterministic fallback", () => {
  const first = {
    id: "first",
    name: "First",
    extensions: ["tar.gz"],
    domains: [],
    directory: "D:\\First",
  };
  const categories = [first, ...DEFAULT_CATEGORIES];
  assert.equal(
    routeCategory(categories, "https://example.test", "archive.tar.gz"),
    first,
  );
  assert.equal(
    routeCategory(categories.slice(1), "https://example.test", "archive.tar.gz")
      ?.id,
    "compressed",
  );
  assert.equal(
    routeCategory([first], "https://example.test", "archive.gz"),
    undefined,
  );
  assert.equal(
    routeCategory(
      [{ ...first, extensions: [""] }],
      "https://example.test",
      "file.",
    ),
    undefined,
  );
});
