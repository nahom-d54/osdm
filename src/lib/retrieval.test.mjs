import test from "node:test";
import assert from "node:assert/strict";
import { authorizedOrigins, mediaSource, qualityLabel } from "./retrieval.ts";

test("media origins require explicit canonical authorization", () => {
  assert.deepEqual(authorizedOrigins("https://cdn.example.test\nhttps://cdn.example.test"), ["https://cdn.example.test"]);
  for (const value of ["https://cdn.example.test/", "https://cdn.example.test/path", "https://cdn.example.test?token=synthetic", "file:///", "https://user:password@example.test"]) assert.throws(() => authorizedOrigins(value));
  assert.throws(() => authorizedOrigins(Array(33).fill("https://cdn.example.test").join("\n")));
});
test("media sources reject unsafe URLs and diagnostics do not expose them", () => {
  for (const value of ["file:///media", "https://user:password@example.test", "https://example.test/#fragment", " https://example.test", "https://example.test/\nsynthetic-secret"]) {
    assert.throws(() => mediaSource(value), (error) => !error.message.includes(value) && !error.message.includes("synthetic-secret"));
  }
  assert.equal(mediaSource("https://media.example.test/video.m3u8?token=synthetic"), "https://media.example.test/video.m3u8?token=synthetic");
});
test("quality labels use declared metadata, never invented measurements", () => {
  assert.match(qualityLabel({ bandwidth: "800000", resolution: [640, 360] }), /640 × 360.*800,000 bit\/s declared/);
  assert.match(qualityLabel({ bandwidth: "1", resolution: null }), /Resolution not declared/);
  assert.ok(qualityLabel({ bandwidth: "18446744073709551615", resolution: null }).includes(BigInt("18446744073709551615").toLocaleString()));
});
