import test from "node:test";
import assert from "node:assert/strict";

async function harness({
  connected = true,
  canceled = false,
  resumed = false,
  failPrepare = false,
  failCancel = false,
  browserType = "firefox",
} = {}) {
  const calls = [];
  let onCreated;
  const listener = () => ({ addListener() {} });
  const api = {
    contextMenus: { onClicked: listener() },
    runtime: {
      onInstalled: listener(),
      onMessage: listener(),
      async sendNativeMessage(_host, msg) {
        calls.push(msg.op);
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
          return { enabled: true, extensions: "zip" };
        },
      },
      session: {
        async get() {
          return { bypassUntil: 0 };
        },
      },
    },
    downloads: {
      onCreated: {
        addListener(fn) {
          onCreated = fn;
        },
      },
      async pause() {
        calls.push("pause");
      },
      async resume() {
        calls.push("resume");
      },
      async cancel() {
        calls.push("cancel");
        if (failCancel) throw new Error("cannot cancel");
      },
      async search() {
        return [{ state: canceled || (browserType === "firefox" && !resumed) ? "interrupted" : "in_progress", paused: !canceled && !resumed }];
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
  await onCreated({
    id: 7,
    url: "https://files.example/a.zip",
    state: "in_progress",
  });
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
}
