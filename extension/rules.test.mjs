import test from "node:test";
import assert from "node:assert/strict";
import { eligible, normalizedLinks } from "./rules.js";
import { captureDefaults } from "./defaults.js";
import { mediaDefaults } from "./media.js";

test("capture, URL-scoped cookies, floating controls and network discovery default on", () => {
  assert.equal(captureDefaults.enabled, true);
  assert.equal(captureDefaults.cookiesEnabled, true);
  assert.equal(mediaDefaults.mediaEnabled, true);
  assert.equal(mediaDefaults.floatingVideos, true);
  assert.equal(mediaDefaults.networkMediaEnabled, true);
});
const rules = {
  enabled: true,
  exclusions: "private.example",
  extensions: "zip tar.gz",
};
test("interception respects opt-out, exclusions, modifiers and file types", () => {
  assert.equal(
    eligible(
      "https://files.example/a.ZIP",
      "https://files.example/page",
      rules,
    ),
    true,
  );
  assert.equal(eligible("https://files.example/a.ZIP", "", rules), false);
  assert.equal(
    eligible(
      "https://files.example/a.zip",
      "https://sub.private.example/page",
      rules,
    ),
    false,
  );
  assert.equal(
    eligible(
      "https://notprivate.example/a.zip",
      "https://notprivate.example/page",
      rules,
    ),
    true,
  );
  assert.equal(
    eligible("https://files.example/a.tar.gz", "", rules, true),
    false,
  );
  assert.equal(eligible("https://files.example/a.html", "", rules), false);
  assert.equal(
    eligible("https://files.example/a.zip", "", { ...rules, enabled: false }),
    false,
  );
  assert.equal(eligible("blob:https://files.example/abc", "", rules), false);
});
test("link selection drops unsupported and credential URLs, deduplicates normalized links", () => {
  assert.deepEqual(
    normalizedLinks([
      "https://FILES.example/a#one",
      "https://files.example/a#two",
      "javascript:void(0)",
      "https://u:p@files.example/a",
    ]),
    ["https://files.example/a"],
  );
});

test("browser filenames enable extensionless capture without bypassing exclusions, URL safety or opt-in", () => {
  const rules = { enabled: true, extensions: "exe", exclusions: "private.example" };
  assert.equal(eligible("https://files.example/download", "https://page.example/", rules, false, "C:\\Downloads\\setup.EXE"), true);
  assert.equal(eligible("https://files.example/download", "https://private.example/", rules, false, "setup.exe"), false);
  assert.equal(eligible("https://files.example/download", "", rules, false, "setup.exe"), false);
  assert.equal(eligible("blob:https://files.example/id", "https://page.example/", rules, false, "setup.exe"), false);
  assert.equal(eligible("https://files.example/download", "https://page.example/", { ...rules, enabled: false }, false, "setup.exe"), false);
});
