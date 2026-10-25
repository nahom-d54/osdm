// Added only to disposable test copies. Never shipped in extension packages.
import "./background.js";

const api = globalThis.browser ?? chrome;
const config = globalThis.OSDM_NATIVE_SMOKE;
const native = async (message) => {
  try { return await api.runtime.sendNativeMessage("app.osdm.capture", message); }
  catch { return { ok: false }; }
};
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const check = (condition, message) => { if (!condition) throw new Error(message); };
const report = async (stage, detail) => {
  await fetch(`${config.base}/report`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ target: config.target, nonce: config.nonce, stage, detail }) });
};
async function waitFor(checker, label, timeout = 30000) {
  const until = Date.now() + timeout;
  do { const value = await checker(); if (value) return value; await delay(300); } while (Date.now() < until);
  throw new Error(`Timed out: ${label}`);
}
async function phase() { return (await (await fetch(`${config.base}/control/${config.target}`)).json()).phase; }
async function transfer(suffix, headers = null) {
  const filename = `native-${config.target}-${suffix}.bin`;
  const prepared = await native({ op: "prepare", url: `${config.base}/${filename}`, filename, headers, explicit_credentials: !!headers });
  check(prepared.ok && !prepared.duplicate, "Native prepare did not create an isolated entry");
  check((await native({ op: "status", id: prepared.id })).pending, "Prepared capture was not pending");
  check((await native({ op: "commit", id: prepared.id })).ok, "Native commit failed");
  const complete = await waitFor(async () => {
    const status = await native({ op: "status", id: prepared.id });
    check(status.state !== "Failed", "Committed transfer failed");
    return status.state === "Completed" && status;
  }, "native publication");
  check(complete.sha256 === config.sha256, "Published bytes did not match the fixture hash");
}
async function browserDownload(suffix, expected) {
  const id = await api.downloads.download({ url: `${config.base}/native-${config.target}-${suffix}.bin`, saveAs: false, conflictAction: "uniquify" });
  await waitFor(async () => {
    const current = (await api.downloads.search({ id }))[0];
    if (expected === "canceled") {
      check(current?.state !== "complete", "Browser completed instead of handing off to OSDM");
      // Both browsers may retain paused=true after cancellation. The terminal
      // error distinguishes cancellation from Firefox's interrupted pause.
      return current?.state === "interrupted" && current.error === "USER_CANCELED";
    }
    check(current?.state !== "interrupted", "Browser fallback was interrupted");
    return current?.state === "complete";
  }, `browser ${expected}`);
}

async function main() {
  await report("browser_version", navigator.userAgent.match(/(?:Chrome|Firefox)\/[0-9.]+/)?.[0] ?? "unknown");
  await report("started", await phase());
  await api.storage.local.set({ enabled: true, extensions: "bin", exclusions: "" });
  if (await phase() === "missing") {
    check(!(await native({ op: "ping" })).ok, "Missing-host test still found a native host");
    await browserDownload("fallback", "complete");
    await report("missing_pass");
    return;
  }
  await waitFor(async () => (await native({ op: "ping" })).ok, "initial native handshake");
  await api.storage.local.set({ enabled: false });
  const diagnostic = await api.downloads.download({ url: `${config.base}/native-${config.target}-pause.bin`, saveAs: false });
  await api.downloads.pause(diagnostic);
  const paused = (await api.downloads.search({ id: diagnostic }))[0];
  await report("paused_state", JSON.stringify({ state: paused.state, paused: paused.paused }));
  await api.downloads.cancel(diagnostic);
  await api.storage.local.set({ enabled: true });
  const pending = await native({ op: "prepare", url: `${config.base}/native-${config.target}-abort.bin`, filename: `native-${config.target}-abort.bin`, headers: null, explicit_credentials: false });
  check(pending.ok, "Abort fixture prepare failed");
  check((await native({ op: "abort", id: pending.id })).ok, "Abort failed");
  check(!(await native({ op: "status", id: pending.id })).ok, "Aborted capture left a phantom entry");
  await transfer("explicit");
  await transfer("private", { cookie: "session=native-fixture", authorization: "Bearer native-fixture", user_agent: null });
  await browserDownload("auto", "canceled");
  await report("initial_pass");
  await waitFor(async () => await phase() === "offline", "app termination phase");
  check(!(await native({ op: "ping" })).ok, "Stopped app still answered native requests");
  await report("offline_pass");
  await waitFor(async () => await phase() === "reconnect", "app restart phase");
  await waitFor(async () => (await native({ op: "ping" })).ok, "native reconnect after app restart");
  await transfer("restart");
  await report("reconnect_pass");
}
void main().catch(async (error) => { await report("failed", String(error.message).replace(/https?:\/\/\S+/g, "[redacted]")).catch(() => {}); });
