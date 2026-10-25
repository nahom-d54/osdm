import test from "node:test";
import assert from "node:assert/strict";
import { eligible, normalizedLinks } from "./rules.js";
const rules = {
  enabled: true,
  exclusions: "private.example",
  extensions: "zip tar.gz",
};
test("interception is opt-in, respects exclusions, modifiers and file types", () => {
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
