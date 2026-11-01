import test from "node:test";
import assert from "node:assert/strict";
import { automaticCookieHeaders, cookieOrigin, cookiePermission, readCookieHeaders } from "./cookies.js";

function fixture(cookies = [{ name: "session", value: "synthetic-test-session", path: "/" }]) {
  const queries = [];
  const permissions = [];
  const api = { permissions: { contains: async (permission) => { permissions.push(permission); return true; } }, cookies: {
    getAll: async (query) => { queries.push(query); return structuredClone(cookies); },
    getAllCookieStores: async () => [{ id: "container-fixture", tabIds: [4] }],
  } };
  return { api, queries, permissions };
}
test("automatic cookies are on by default for HTTPS with a global opt-out, not a per-site approval list", async () => {
  const { api, queries, permissions } = fixture();
  for (const url of ["https://other.example/file", "https://sub.login.example/file", "https://login.example:8443/file"]) {
    assert.equal((await automaticCookieHeaders(api, url, {}, {})).cookie, "session=synthetic-test-session");
  }
  const before = queries.length;
  assert.equal(await automaticCookieHeaders(api, "https://login.example/file", { cookiesEnabled: false }, {}), null);
  assert.equal(await automaticCookieHeaders(api, "http://login.example/file", {}, {}), null);
  assert.equal(queries.length, before);
  assert.deepEqual(permissions[0], { permissions: ["cookies"], origins: ["https://*/*"] });
  assert.equal(cookieOrigin("https://user:secret@login.example"), null);
  assert.deepEqual(cookiePermission("https://login.example:8443/path"), { permissions: ["cookies"], origins: ["https://login.example/*"] });
});
test("only URL-matching cookies are requested from the known container and sent as session-only captured headers", async () => {
  const { api, queries } = fixture();
  const headers = await automaticCookieHeaders(api, "https://login.example/file.exe", { cookiesEnabled: true }, { cookieStoreId: "container-fixture" });
  assert.equal(headers.cookie, "session=synthetic-test-session");
  assert.deepEqual(queries, [{ url: "https://login.example/file.exe", storeId: "container-fixture" }]);
  assert.equal(headers.authorization, null);
});
test("revoked cookie permission leaves the browser download available instead of attempting anonymous handoff", async () => {
  const { api, queries } = fixture(); api.permissions.contains = async () => false;
  await assert.rejects(automaticCookieHeaders(api, "https://login.example/file", { cookiesEnabled: true }, {}));
  assert.equal(queries.length, 0);
});
test("private cookie stores are never guessed; explicit context capture selects its actual tab store", async () => {
  const { api, queries } = fixture();
  await assert.rejects(readCookieHeaders(api, "https://login.example/file", { incognito: true }));
  assert.equal(queries.length, 0);
  await readCookieHeaders(api, "https://login.example/file", { tab: { id: 4, incognito: true } });
  assert.equal(queries[0].storeId, "container-fixture");
});
test("partitioned contexts, oversized headers and header injection fail closed without echoing cookie values", async () => {
  for (const cookies of [
    [{ name: "session", value: "synthetic", partitionKey: { topLevelSite: "https://page.example" } }],
    [{ name: "session", value: "synthetic\r\nInjected: yes" }],
    [{ name: "bad=name", value: "synthetic" }],
    [{ name: "session", value: "x".repeat(16385) }],
    Array.from({ length: 257 }, () => ({ name: "session", value: "synthetic" })),
  ]) {
    const { api } = fixture(cookies);
    await assert.rejects(readCookieHeaders(api, "https://login.example/file"), (error) => !error.message.includes("synthetic"));
  }
});
test("same-name cookies retain longest-path-first order and empty jars add no credential header", async () => {
  const { api } = fixture([{ name: "session", value: "root-fixture", path: "/" }, { name: "session", value: "path-fixture", path: "/files" }]);
  assert.equal((await readCookieHeaders(api, "https://login.example/files/a")).cookie, "session=path-fixture; session=root-fixture");
  assert.equal(await readCookieHeaders(fixture([]).api, "https://login.example/file"), null);
});
