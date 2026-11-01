import test from "node:test";
import assert from "node:assert/strict";
import {
  canonicalUrl,
  capturedRequestHeaders,
  findRequest,
  rememberRequest,
  replayableHeaders,
  validUserAgent,
} from "./request-headers.js";

test("only non-secret replayable headers survive with one value per name", () => {
  const pairs = replayableHeaders([
    { name: "User-Agent", value: "Browser/1.0" },
    { name: "Cookie", value: "session=secret" },
    { name: "Authorization", value: "Bearer secret" },
    { name: "Accept-Language", value: "en-US,en;q=0.9" },
    { name: "accept", value: "video/*" },
    { name: "Accept", value: "duplicate" },
    { name: "Referer", value: "https://page.example/watch?token=transient" },
    { name: "Range", value: "bytes=0-1" },
    { name: "If-None-Match", value: "\"etag\"" },
    { name: "Sec-CH-UA", value: "\"Chromium\";v=\"156\"" },
  ]);
  assert.deepEqual(pairs, [
    ["accept-language", "en-US,en;q=0.9"],
    ["accept", "video/*"],
    ["sec-ch-ua", "\"Chromium\";v=\"156\""],
  ]);
});

test("header injection, control characters and oversized values are discarded", () => {
  assert.deepEqual(replayableHeaders([
    { name: "accept", value: "text/html\r\nX-Evil: 1" },
    { name: "referer", value: "https://page.example/\u0000" },
    { name: "origin", value: "x".repeat(2049) },
    { name: "sec-ch-ua-platform", value: "\"Windows\"" },
  ]), [["sec-ch-ua-platform", "\"Windows\""]]);
  assert.deepEqual(capturedRequestHeaders("session=ok", [["accept", "*/*"]], "Browser/1.0").headers, [{ name: "accept", value: "*/*" }]);
  assert.equal(capturedRequestHeaders(null, [], "Browser/1.0").user_agent, "Browser/1.0");
  for (const value of ["", "x".repeat(513), "a\r\nb", 42]) assert.equal(validUserAgent(value), false);
});

test("remembered requests are canonical, bounded, expiring and never include a query secret copy", () => {
  const map = new Map();
  rememberRequest(map, "https://files.example/a.zip?token=transient#fragment", [{ name: "accept-language", value: "en-GB" }], 1000);
  const key = canonicalUrl("https://files.example/a.zip?token=transient#fragment");
  assert.equal(key, "https://files.example/a.zip?token=transient");
  assert.deepEqual(findRequest(map, key, 2000), [["accept-language", "en-GB"]]);
  assert.equal(findRequest(map, key, 1000 + 5 * 60 * 1000 + 1), null);
  assert.equal(map.has(key), false);
  for (let index = 0; index < 300; index++) rememberRequest(map, `https://files.example/${index}.bin`, [{ name: "accept", value: "*/*" }], 1000);
  assert.equal(map.size, 256);
  assert.equal(findRequest(map, "https://files.example/0.bin", 1000), null);
  assert.equal(canonicalUrl("blob:https://files.example/id"), null);
  assert.equal(canonicalUrl("https://user:pass@files.example/a"), null);
});

test("capture envelope bounds pairs and keeps the browser identity separate", () => {
  const pairs = Array.from({ length: 20 }, () => ["accept", "*/*"]);
  const captured = capturedRequestHeaders("session=ok", pairs, "Browser/1.0");
  assert.equal(captured.cookie, "session=ok");
  assert.equal(captured.authorization, null);
  assert.equal(captured.user_agent, "Browser/1.0");
  assert.equal(captured.headers.length, 16);
  assert.equal(capturedRequestHeaders("", [], "bad\r\nagent").user_agent, null);
});
