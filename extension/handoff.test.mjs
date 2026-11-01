import test from "node:test";
import assert from "node:assert/strict";

async function harness({
  connected = true,
  canceled = false,
  resumed = false,
  failPrepare = false,
  failCancel = false,
  browserType = "firefox",
  item = { id: 7, url: "https://files.example/a.zip", state: "in_progress" },
  lateFilename,
  determiningFilename = false,
  messages = [],
  cookiesEnabled = false,
  cookiePermissionGranted = true,
  cookieQueries = [],
} = {}) {
  const calls = [];
  let onCreated;
  let onChanged;
  let onFilename;
  let paused = false;
  const listener = () => ({ addListener() {} });
  const api = {
    permissions: { contains: async () => cookiePermissionGranted },
    cookies: { getAll: async (query) => { cookieQueries.push(query); calls.push("cookies"); return [{ name: "session", value: "synthetic-test-session", path: "/" }]; } },
    contextMenus: { onClicked: listener() },
    runtime: {
      onInstalled: listener(),
      onMessage: listener(),
      async sendNativeMessage(_host, msg) {
        calls.push(msg.op);
        messages.push(msg);
        if (!connected) throw new Error("host missing");
        if (msg.op === "ping") return { ok: true, version: 1 };
        if (msg.op === "prepare")
          return failPrepare
            ? { ok: false }
            : { ok: true, id: "fixture-id", duplicate: false };
        return { ok: true };
      },
    },
    storage: {
      local: {
        async get() {
          return { enabled: true, extensions: "zip exe", cookiesEnabled };
        },
      },
      session: {
        async set() {},
        async get() {
          return { bypassUntil: 0 };
        },
      },
    },
    downloads: {
      onChanged: { addListener(fn) { onChanged = fn; } },
      ...(browserType === "chromium" ? { onDeterminingFilename: { addListener(fn) { onFilename = fn; } } } : {}),
      onCreated: {
        addListener(fn) {
          onCreated = fn;
        },
      },
      async pause() {
        calls.push("pause");
        paused = true;
      },
      async resume() {
        calls.push("resume");
      },
      async cancel() {
        calls.push("cancel");
        if (failCancel) throw new Error("cannot cancel");
      },
      async search() {
        return [{ ...item, state: canceled || (paused && browserType === "firefox" && !resumed) ? "interrupted" : "in_progress", paused: paused && !canceled && !resumed }];
      },
    },
    notifications: {
      async create() {
        calls.push("notify");
      },
    },
  };
  if (browserType === "chromium") globalThis.chrome = api;
  else globalThis.browser = api;
  await import(`./background.js?test=${Math.random()}`);
  const initial = onCreated(item);
  if (determiningFilename) {
    item = { ...item, filename: lateFilename };
    await new Promise((resolve) => { assert.equal(onFilename(item, resolve), true); });
  }
  await initial;
  if (lateFilename && !determiningFilename) {
    item = { ...item, filename: lateFilename };
    await onChanged({ id: item.id, filename: { current: lateFilename } });
  }
  delete globalThis.browser;
  delete globalThis.chrome;
  return calls;
}

for (const browserType of ["chromium", "firefox"]) {
  test(`${browserType}: missing host leaves browser downloader untouched`, async () => {
    assert.deepEqual(await harness({ browserType, connected: false }), [
      "ping",
    ]);
  });
  test(`${browserType}: handoff prepares durable intent before cancellation and commit`, async () => {
    assert.deepEqual(await harness({ browserType }), [
      "ping",
      "pause",
      "prepare",
      "cancel",
      "commit",
    ]);
  });
  test(`${browserType}: prepare failure resumes browser`, async () => {
    assert.deepEqual(await harness({ browserType, failPrepare: true }), [
      "ping",
      "pause",
      "prepare",
      "resume",
    ]);
  });
  test(`${browserType}: user cancellation aborts prepared intent without commit`, async () => {
    assert.deepEqual(await harness({ browserType, canceled: true }), [
      "ping",
      "pause",
      "prepare",
      "abort",
    ]);
  });
  test(`${browserType}: browser cancellation failure aborts and falls back`, async () => {
    assert.deepEqual(await harness({ browserType, failCancel: true }), [
      "ping",
      "pause",
      "prepare",
      "cancel",
      "abort",
      "resume",
    ]);
  });
  test(`${browserType}: user resume aborts handoff without canceling the browser`, async () => {
    assert.deepEqual(await harness({ browserType, resumed: true }), ["ping", "pause", "prepare", "abort"]);
  });
  test(`${browserType}: Content-Disposition EXE filename captures an extensionless URL`, async () => {
    const messages = [];
    assert.deepEqual(await harness({ browserType, item: { id: 7, url: "https://files.example/download?id=42", filename: "C:\\Downloads\\Installer.EXE", state: "in_progress" }, messages }), ["ping", "pause", "prepare", "cancel", "commit"]);
    assert.equal(messages.find((message) => message.op === "prepare").filename, "Installer.EXE");
  });
  test(`${browserType}: late filename detection retries the previously ineligible URL once`, async () => {
    assert.deepEqual(await harness({ browserType, item: { id: 7, url: "https://files.example/download", state: "in_progress" }, lateFilename: "installer.exe" }), ["ping", "pause", "prepare", "cancel", "commit"]);
  });
}

test("Chromium filename callback waits for a richer filename handoff without duplicate preparation", async () => {
  assert.deepEqual(await harness({ browserType: "chromium", item: { id: 7, url: "https://files.example/download", state: "in_progress" }, lateFilename: "installer.exe", determiningFilename: true }), ["ping", "pause", "prepare", "cancel", "commit"]);
});

test("automatic file capture passes only URL-matching cookies through the explicit credential protocol", async () => {
  const messages = []; const cookieQueries = [];
  assert.deepEqual(await harness({ browserType: "chromium", cookiesEnabled: true, messages, cookieQueries }), ["ping", "cookies", "pause", "prepare", "cancel", "commit"]);
  const prepared = messages.find((message) => message.op === "prepare");
  assert.equal(prepared.explicit_credentials, true);
  assert.equal(prepared.review, true);
  assert.equal(prepared.headers.cookie, "session=synthetic-test-session");
  assert.equal(typeof prepared.headers.user_agent, "string");
  assert.ok(Array.isArray(prepared.headers.headers));
  assert.deepEqual(cookieQueries, [{ url: "https://files.example/a.zip" }]);
});
test("revoked automatic-cookie consent permission never pauses or cancels the browser download", async () => {
  assert.deepEqual(await harness({ browserType: "chromium", cookiesEnabled: true, cookiePermissionGranted: false }), ["ping"]);
});
