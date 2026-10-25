import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { generateKeyPairSync } from "node:crypto";
import { validateVersion, chromiumId, stageExtension, packageFiles, uploadChromiumDraft } from "./release.mjs";

test("store version validation rejects tags and ambiguous numeric forms", () => {
  assert.equal(validateVersion("1.2.3.4"), "1.2.3.4");
  for (const value of ["v1.2", "1.2-alpha.1", "01.2", "0.0.0", "1.2.3.4.5", "65536"]) assert.throws(() => validateVersion(value));
});

test("release staging excludes native harnesses, profiles and source secrets", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "osdm-package-test-"));
  const { publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const key = publicKey.export({ format: "der", type: "spki" }).toString("base64");
  assert.match(chromiumId(key), /^[a-p]{32}$/);
  const chrome = await stageExtension("chromium", "1.2.3", path.join(root, "chromium"), key);
  assert.equal(chrome.key, key);
  const files = (await fs.readdir(path.join(root, "chromium"))).sort();
  assert.deepEqual(files, [...packageFiles, "manifest.json"].sort());
  const firefox = await stageExtension("firefox", "1.2.3", path.join(root, "firefox"));
  assert.equal(firefox.browser_specific_settings.gecko.id, "osdm@osdm.app");
  await assert.rejects(stageExtension("firefox", "1.2.3", path.join(root, "firefox")));
});

test("Chrome upload is draft-only and never returns OAuth secrets", async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "osdm-draft-test-"));
  const zip = path.join(root, "fixture.zip"); await fs.writeFile(zip, "bounded test package");
  const calls = [];
  const request = async (url, options) => {
    calls.push({ url, method: options.method });
    return new Response(JSON.stringify(calls.length === 1 ? { access_token: "fixture-private-token" } : { uploadState: "SUCCEEDED" }));
  };
  const result = await uploadChromiumDraft({ zip, publisher: "fixture-publisher", item: "a".repeat(32), credentials: { clientId: "fixture", clientSecret: "fixture-secret", refreshToken: "fixture-refresh" } }, request);
  assert.deepEqual(result, { uploaded_draft: true, published: false });
  assert.equal(calls.length, 2);
  assert.ok(calls.every((call) => !call.url.includes(":publish")));
  assert.ok(!JSON.stringify(result).includes("private-token"));
});
