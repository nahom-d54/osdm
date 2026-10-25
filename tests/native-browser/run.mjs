import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import http from "node:http";
import { createHash, generateKeyPairSync, randomBytes } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import webExt from "web-ext";
import { chromium } from "@playwright/test";

if (process.platform !== "win32") throw new Error("This registration harness currently requires Windows");
const options = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, i, all) => i % 2 ? pairs : [...pairs, [value.replace(/^--/, ""), all[i + 1]]], []));
const repo = fileURLToPath(new URL("../../", import.meta.url));
const parent = path.join(process.env.LOCALAPPDATA ?? os.tmpdir(), "Temp", "opencode");
await fs.mkdir(parent, { recursive: true });
const root = await fs.mkdtemp(path.join(parent, "osdm-native-browser-"));
const data = path.join(root, "app-data");
await fs.mkdir(data);
await fs.writeFile(path.join(data, ".osdm-native-test"), "OSDM native test v1\n");
await fs.writeFile(path.join(data, "capture-enabled"), "enabled");
const env = { ...process.env, OSDM_NATIVE_TEST_DATA: data, TMP: root, TEMP: root };
const registration = path.join(repo, "tests/native-browser/registration.ps1");
const stateFile = path.join(root, "registration-backup.json");
const manifests = { chromium: path.join(root, "chromium-host.json"), firefox: path.join(root, "firefox-host.json") };
const body = Buffer.alloc(128 * 1024, 71);
const sha256 = createHash("sha256").update(body).digest("hex");
const nonce = randomBytes(24).toString("hex");
const results = [];
const stages = new Map();
const phases = new Map();
const processes = new Set();
let app;
let runner;
let registryTouched = false;
let leaks = 0;
let aborted = false;
process.once("SIGINT", () => { aborted = true; });
process.once("SIGTERM", () => { aborted = true; });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
function check(value, message) { if (!value) throw new Error(message); }
async function waitFor(predicate, label, ms = 60000) {
  const end = Date.now() + ms;
  do { check(!aborted, "Native test interrupted"); const value = await predicate(); if (value) return value; await sleep(200); } while (Date.now() < end);
  throw new Error(`Timed out: ${label}`);
}
function ps(args) {
  const result = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", registration, ...args], { cwd: repo, encoding: "utf8", timeout: 30000 });
  check(result.status === 0, "Temporary registration/restore failed; use the retained backup to restore manually");
}
async function kill(child) {
  if (!child || child.exitCode != null) return;
  spawnSync("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", timeout: 15000 });
  await waitFor(() => child.exitCode != null || child.signalCode != null, "owned process termination", 15000);
  processes.delete(child);
}
async function launchApp() {
  app = spawn(options.app, [], { cwd: root, env, stdio: ["ignore", "pipe", "pipe"] });
  const log = path.join(root, `app-${Date.now()}.log`);
  for (const stream of [app.stdout, app.stderr]) stream.on("data", (bytes) => { void fs.appendFile(log, bytes).catch(() => {}); });
  processes.add(app);
  await waitFor(async () => {
    if (app.exitCode != null) throw new Error("Isolated desktop app exited before becoming ready");
    try { await fs.access(path.join(data, "capture-bridge.bin")); return true; } catch { return false; }
  }, "isolated app endpoint");
}
function preflightBinary(exe, framed) {
  const result = spawnSync(exe, ["--native-test-info"], { cwd: root, env, input: Buffer.alloc(0), timeout: 10000 });
  check(result.status === 0 && result.stdout?.length, "Native-test diagnostic failed; build current debug binaries first");
  const bytes = result.stdout;
  const text = framed ? bytes.subarray(4, 4 + bytes.readUInt32LE(0)).toString() : bytes.toString().trim();
  check(JSON.parse(text).native_test === true, "Refusing to run a binary without debug isolation");
}
async function writeHosts(chromeId, missing = false) {
  const common = { name: "app.osdm.capture", description: "OSDM disposable native test", path: missing ? path.join(root, "not-installed.exe") : path.resolve(options.host), type: "stdio" };
  await fs.writeFile(manifests.chromium, JSON.stringify({ ...common, allowed_origins: [`chrome-extension://${chromeId}/`] }));
  await fs.writeFile(manifests.firefox, JSON.stringify({ ...common, allowed_extensions: ["osdm@osdm.app"] }));
}
function serveBytes(request, response, target = false) {
  if (target && (request.headers.cookie || request.headers.authorization)) leaks++;
  response.setHeader("Access-Control-Allow-Origin", "*");
  response.setHeader("Content-Type", "application/octet-stream");
  response.setHeader("ETag", '"native-fixture-v1"');
  if (request.headers.range === "bytes=0-0") {
    response.writeHead(206, { "Content-Range": `bytes 0-0/${body.length}`, "Content-Length": 1 }); response.end(body.subarray(0, 1)); return;
  }
  response.setHeader("Content-Length", body.length);
  let offset = 0;
  const timer = setInterval(() => { response.write(body.subarray(offset, offset + 4096)); offset += 4096; if (offset >= body.length) { clearInterval(timer); response.end(); } }, 100);
  response.on("close", () => clearInterval(timer));
}
const secondary = http.createServer((req, res) => serveBytes(req, res, true));
await new Promise((resolve) => secondary.listen(0, "127.0.0.1", resolve));
const server = http.createServer(async (req, res) => {
  try {
  const url = new URL(req.url, "http://127.0.0.1");
  res.setHeader("Access-Control-Allow-Origin", "*"); res.setHeader("Access-Control-Allow-Headers", "Content-Type");
  if (req.method === "OPTIONS") { res.end(); return; }
  if (url.pathname === "/report") {
    let bytes = "";
    for await (const chunk of req) { bytes += chunk; if (bytes.length > 8192) { res.writeHead(413).end(); return; } }
    const event = JSON.parse(bytes);
    if (event.nonce !== nonce) { res.writeHead(403).end(); return; }
    stages.set(`${event.target}:${event.stage}`, true);
    results.push({ target: event.target, stage: event.stage, ...(typeof event.detail === "string" ? { detail: event.detail.slice(0, 250) } : {}) }); res.end("ok"); return;
  }
  if (url.pathname.startsWith("/control/")) { res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify({ phase: phases.get(url.pathname.split("/").at(-1)) ?? "initial" })); return; }
  if (url.pathname.endsWith("-private.bin")) {
    if (req.headers.cookie !== "session=native-fixture" || req.headers.authorization !== "Bearer native-fixture") { res.writeHead(401).end(); return; }
    res.writeHead(302, { Location: `http://127.0.0.1:${secondary.address().port}/fixture.bin` }); res.end(); return;
  }
  serveBytes(req, res);
  } catch { if (!res.headersSent) res.writeHead(400); res.end(); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const { publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const key = publicKey.export({ type: "spki", format: "der" });
const chromeId = [...createHash("sha256").update(key).digest().subarray(0, 16)].map((v) => String.fromCharCode(97 + (v >> 4), 97 + (v & 15))).join("");

async function prepareExtension(target) {
  const source = path.join(root, `${target}-extension`); await fs.mkdir(source);
  for (const name of ["background.js", "rules.js", "modifiers.js", "options.html", "options.js", "icon.svg"]) await fs.copyFile(path.join(repo, "extension", name), path.join(source, name));
  const manifest = JSON.parse(await fs.readFile(path.join(repo, `extension/manifest.${target}.json`), "utf8"));
  if (target === "chromium") { manifest.key = key.toString("base64"); manifest.background.service_worker = "native-smoke.js"; }
  else manifest.background.scripts = ["native-smoke.js"];
  manifest.host_permissions = ["http://127.0.0.1/*"];
  const config = { target, nonce, base, sha256 };
  const smoke = `globalThis.OSDM_NATIVE_SMOKE=${JSON.stringify(config)};\n${await fs.readFile(path.join(repo, "tests/native-browser/smoke.js"), "utf8")}`;
  await fs.writeFile(path.join(source, "native-smoke.js"), smoke);
  await fs.writeFile(path.join(source, "manifest.json"), JSON.stringify(manifest, null, 2));
  return source;
}
async function launchBrowser(target, source) {
  const profile = path.join(root, `${target}-profile`); await fs.mkdir(profile, { recursive: true });
  const downloads = path.join(root, `${target}-downloads`); await fs.mkdir(downloads, { recursive: true });
  const savedEnvironment = process.env.OSDM_NATIVE_TEST_DATA;
  process.env.OSDM_NATIVE_TEST_DATA = data;
  // web-ext launches children from process.env. Its profiles and all temporary
  // files are owned by this run; no normal browser profile is opened or copied.
  process.env.TMP = root; process.env.TEMP = root;
  try {
    if (target === "chromium") {
      await fs.mkdir(path.join(profile, "Default"), { recursive: true });
      await fs.writeFile(path.join(profile, "Default/Preferences"), JSON.stringify({ download: { default_directory: downloads, prompt_for_download: false }, browser: { check_default_browser: false } }));
      const context = await chromium.launchPersistentContext(profile, {
        executablePath: options.chrome ?? "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        headless: true, env, acceptDownloads: true, downloadsPath: downloads,
        ignoreDefaultArgs: ["--disable-extensions"],
        args: ["--no-first-run", "--no-default-browser-check", "--enable-unsafe-extension-debugging"],
      });
      runner = { exit: () => context.close() };
      const session = await context.browser().newBrowserCDPSession();
      await session.send("Extensions.loadUnpacked", { path: source });
      // An already-installed MV3 extension need not start its worker at browser
      // launch. A real extension message wakes the persisted listener reliably.
      const page = await context.newPage();
      await page.goto(`chrome-extension://${chromeId}/options.html`);
      await page.evaluate(() => chrome.runtime.sendMessage({ op: "__native_test_wake" }).catch(() => {}));
      await page.close();
    } else {
      const prefs = { "browser.download.folderList": 2, "browser.download.dir": downloads, "browser.download.useDownloadDir": true, "browser.helperApps.neverAsk.saveToDisk": "application/octet-stream", "browser.download.alwaysOpenPanel": false };
      await fs.writeFile(path.join(profile, "user.js"), Object.entries(prefs).map(([k, v]) => `user_pref(${JSON.stringify(k)},${JSON.stringify(v)});`).join("\n"));
      runner = await webExt.cmd.run({ target: ["firefox-desktop"], firefox: options.firefox ?? "C:\\Program Files\\Mozilla Firefox\\firefox.exe", firefoxProfile: profile, keepProfileChanges: true, sourceDir: source, artifactsDir: root, noInput: true, noReload: true, reload: false, startUrl: ["about:blank"], args: ["-headless"], pref: Object.entries(prefs).map(([k,v])=>`${k}=${v}`) }, { shouldExitProgram: false });
    }
  } finally { if (savedEnvironment == null) delete process.env.OSDM_NATIVE_TEST_DATA; else process.env.OSDM_NATIVE_TEST_DATA = savedEnvironment; }
}
async function stage(target, name) {
  await waitFor(() => { check(!stages.has(`${target}:failed`), `${target} reported a native test failure`); return stages.has(`${target}:${name}`); }, `${target} ${name}`);
}
async function verifyFile(file) {
  await waitFor(async () => { try { return createHash("sha256").update(await fs.readFile(file)).digest("hex") === sha256; } catch { return false; } }, "byte-exact fixture publication");
}
async function verifyBrowserFile(target) {
  const directory = path.join(root, `${target}-downloads`);
  await waitFor(async () => {
    for (const name of await fs.readdir(directory)) {
      const file = path.join(directory, name);
      const stat = await fs.stat(file);
      if (stat.isFile() && stat.size === body.length && createHash("sha256").update(await fs.readFile(file)).digest("hex") === sha256) return true;
    }
    return false;
  }, "byte-exact browser fallback fixture");
}

let failure;
try {
  check(options.app && options.host, "Supply built --app and --host paths");
  // Reject old, uninstrumented executables before invoking diagnostics.
  for (const exe of [options.app, options.host]) {
    const bytes = await fs.readFile(exe);
    check(bytes.includes(Buffer.from("--native-test-info")), "Binary predates native-test isolation; rebuild before testing");
  }
  preflightBinary(options.app, false); preflightBinary(options.host, true);
  await writeHosts(chromeId);
  registryTouched = true;
  ps(["-Mode", "Install", "-StateFile", stateFile, "-ChromiumManifest", manifests.chromium, "-FirefoxManifest", manifests.firefox]);
  await launchApp();
  const targets = options.browser === "both" || !options.browser ? ["chromium", "firefox"] : [options.browser];
  for (const target of targets) {
    phases.set(target, "initial"); await writeHosts(chromeId);
    const source = await prepareExtension(target);
    await launchBrowser(target, source);
    await stage(target, "initial_pass");
    await verifyFile(path.join(data, "downloads", `native-${target}-auto.bin`));
    await kill(app); phases.set(target, "offline");
    await stage(target, "offline_pass");
    await launchApp(); phases.set(target, "reconnect");
    await stage(target, "reconnect_pass");
    await runner.exit(); runner = null;
    await writeHosts(chromeId, true); phases.set(target, "missing");
    await launchBrowser(target, source);
    await stage(target, "missing_pass");
    await verifyBrowserFile(target);
    check(!await fs.stat(path.join(data, "downloads", `native-${target}-fallback.bin`)).then(()=>true,()=>false), "Browser fallback left a phantom OSDM file");
    await runner.exit(); runner = null;
  }
  check(leaks === 0, "Captured credentials crossed an origin redirect");
} catch (error) { failure = error.message; }
finally {
  if (runner) { try { await runner.exit(); } catch { failure ??= "Owned browser cleanup failed"; } }
  const cleanup = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(repo, "tests/native-browser/cleanup.ps1"), "-Root", root], { cwd: repo, stdio: "ignore", timeout: 30000 });
  if (cleanup.status !== 0) failure ??= "Owned browser cleanup requires attention";
  for (const child of processes) await kill(child).catch(() => {});
  if (registryTouched && await fs.stat(stateFile).then(()=>true,()=>false)) { try { ps(["-Mode", "Restore", "-StateFile", stateFile]); } catch { failure = "Registration restore requires attention; retained backup must be restored manually"; } }
  server.closeAllConnections(); secondary.closeAllConnections(); server.close(); secondary.close();
  await fs.writeFile(path.join(root, "results.json"), JSON.stringify({ passed: !failure, failure, results, credentials_leaked: leaks, signed_firefox_restart: "not-tested-temporary-addon" }, null, 2));
}
console.log(`Isolated native-browser evidence: ${root}`);
if (failure) { console.error(failure); process.exitCode = 1; }
