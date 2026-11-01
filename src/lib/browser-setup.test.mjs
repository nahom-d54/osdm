import assert from "node:assert/strict";
import test from "node:test";
import { validExtensionId, BROWSER_MANUAL_SETUP } from "./browser-setup.ts";

test("browser helper identities are exact, never arbitrary origins or paths", () => {
  for (const browser of ["chrome", "edge", "brave", "chromium"]) {
    assert.equal(validExtensionId(browser, "abcdefghijklmnopabcdefghijklmnop"), true);
    for (const value of ["", "invalid", "ABCDEFGHIJKLMNOPABCDEFGHIJKLMNOP", "abcdefghijklmnopabcdefghijklmnop/", "chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]) assert.equal(validExtensionId(browser, value), false);
  }
  assert.equal(validExtensionId("firefox", "osdm@osdm.app"), true);
  assert.equal(validExtensionId("firefox", "arbitrary@extension.test"), false);
  assert.match(BROWSER_MANUAL_SETUP.firefox, /Temporary/);
  assert.match(BROWSER_MANUAL_SETUP.firefox, /signed/);
});
