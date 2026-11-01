import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { stageBrowserResources, validateStoreConfiguration } from "../scripts/desktop-build.mjs";
import { packageFiles, chromiumId, bundledIdentity } from "./release.mjs";

test("desktop bundle stages a native helper and only allowlisted browser assets", async () => {
  const base = await fs.mkdtemp(path.join(os.tmpdir(), "osdm-browser-bundle-test-"));
  try {
    const helper = path.join(base, "osdm-native-host.exe"); await fs.writeFile(helper, "synthetic fixture, never executed");
    const output = path.join(base, "fresh");
    const config = JSON.parse(await fs.readFile(await stageBrowserResources(helper, output), "utf8"));
    assert.equal(config.bundle.resources[path.join(output, "osdm-native-host.exe")], "browser-integration/osdm-native-host.exe");
    assert.equal(config.bundle.resources[path.join(output, "extension")], "browser-integration/extension");
    for (const browser of ["chromium", "edge", "firefox"]) {
      const directory = path.join(output, "extension", browser);
      assert.deepEqual((await fs.readdir(directory)).sort(), [...packageFiles, "manifest.json", "ui-assets"].sort());
      const manifest = JSON.parse(await fs.readFile(path.join(directory, "manifest.json"), "utf8"));
      assert.ok(manifest.permissions.includes("nativeMessaging"));
      if (browser === "firefox") assert.equal(manifest.browser_specific_settings.gecko.id, "osdm@osdm.app");
      else assert.equal(chromiumId(manifest.key), JSON.parse(await fs.readFile(path.join(output, "identities.json"), "utf8"))[browser === "edge" ? "edge" : "chrome"]);
    }
    await assert.rejects(stageBrowserResources(helper, output));
    assert.equal(await fs.readFile(helper, "utf8"), "synthetic fixture, never executed");
  } finally { await fs.rm(base, { recursive: true, force: true }); }
});

test("bundled public key gives every installation and profile the same fallback identity", () => {
  assert.equal(chromiumId(bundledIdentity.chromium.public_key), bundledIdentity.chromium.id);
  assert.match(bundledIdentity.chromium.id, /^[a-p]{32}$/);
  assert.equal(Object.keys(bundledIdentity.chromium).some((key) => /private|secret/i.test(key)), false);
});

test("store-ready release gate rejects missing or mismatched listings instead of shipping manual setup", () => {
  assert.throws(() => validateStoreConfiguration({ OSDM_REQUIRE_BROWSER_STORES: "1" }), /Chrome/);
  const config = { OSDM_REQUIRE_BROWSER_STORES: "1", OSDM_CHROME_EXTENSION_ID: bundledIdentity.chromium.id, OSDM_CHROMIUM_PUBLIC_KEY: bundledIdentity.chromium.public_key,
    OSDM_EDGE_EXTENSION_ID: bundledIdentity.chromium.id, OSDM_EDGE_PUBLIC_KEY: bundledIdentity.chromium.public_key, OSDM_FIREFOX_ADDON_SLUG: "osdm-test-fixture" };
  assert.doesNotThrow(() => validateStoreConfiguration(config));
  assert.throws(() => validateStoreConfiguration({ ...config, OSDM_EDGE_EXTENSION_ID: "a".repeat(32) }), /Edge/);
  assert.throws(() => validateStoreConfiguration({ ...config, OSDM_FIREFOX_ADDON_SLUG: "https://example.invalid" }), /Firefox/);
});
