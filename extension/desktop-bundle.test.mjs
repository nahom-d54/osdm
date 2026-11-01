import assert from "node:assert/strict";
import test from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { stageBrowserResources } from "../scripts/desktop-build.mjs";
import { packageFiles } from "./release.mjs";

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
      assert.deepEqual((await fs.readdir(directory)).sort(), [...packageFiles, "manifest.json"].sort());
      const manifest = JSON.parse(await fs.readFile(path.join(directory, "manifest.json"), "utf8"));
      assert.ok(manifest.permissions.includes("nativeMessaging"));
      if (browser === "firefox") assert.equal(manifest.browser_specific_settings.gecko.id, "osdm@osdm.app");
    }
    await assert.rejects(stageBrowserResources(helper, output));
    assert.equal(await fs.readFile(helper, "utf8"), "synthetic fixture, never executed");
  } finally { await fs.rm(base, { recursive: true, force: true }); }
});
