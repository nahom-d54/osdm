import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createHash, createPublicKey } from "node:crypto";
import webExt from "web-ext";

export const packageFiles = ["background.js", "rules.js", "modifiers.js", "options.html", "options.js", "icon.svg"];
const source = fileURLToPath(new URL("./", import.meta.url));

export function validateVersion(version) {
  const parts = version.split(".");
  if (parts.length < 1 || parts.length > 4 || !parts.every((v) => /^(0|[1-9][0-9]*)$/.test(v) && Number(v) <= 65535) || !parts.some((v) => Number(v) > 0)) throw new Error("Use one to four numeric extension-version components (0–65535), not a prerelease tag");
  return version;
}

export function chromiumId(publicKey) {
  const key = createPublicKey({ key: Buffer.from(publicKey, "base64"), format: "der", type: "spki" });
  const der = key.export({ type: "spki", format: "der" });
  return [...createHash("sha256").update(der).digest().subarray(0, 16)].map((v) => String.fromCharCode(97 + (v >> 4), 97 + (v & 15))).join("");
}

export async function stageExtension(browser, version, directory, publicKey) {
  if (!["chromium", "firefox"].includes(browser)) throw new Error("Unknown browser target");
  validateVersion(version);
  const manifest = JSON.parse(await fs.readFile(path.join(source, `manifest.${browser}.json`), "utf8"));
  manifest.version = version;
  if (browser === "chromium" && publicKey) { chromiumId(publicKey); manifest.key = publicKey; }
  // Create a new staging directory, never sweep or overwrite an existing tree.
  await fs.mkdir(directory);
  for (const file of packageFiles) await fs.copyFile(path.join(source, file), path.join(directory, file));
  await fs.writeFile(path.join(directory, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n", { flag: "wx" });
  return manifest;
}

export async function buildPackages(output, version, publicKey) {
  validateVersion(version);
  await fs.mkdir(output); // An existing output requires a new path, not pruning.
  const staging = path.join(output, "source"); await fs.mkdir(staging);
  for (const browser of ["chromium", "firefox"]) {
    const directory = path.join(staging, browser);
    await stageExtension(browser, version, directory, publicKey);
    await webExt.cmd.build({ sourceDir: directory, artifactsDir: output, filename: `osdm-capture-${browser}-${version}.zip`, overwriteDest: false }, { shouldExitProgram: false });
  }
  await writeChecksums(output);
}

export async function writeChecksums(output) {
  const names = (await fs.readdir(output)).filter((name) => /\.(zip|xpi)$/.test(name)).sort();
  const lines = [];
  for (const name of names) lines.push(`${createHash("sha256").update(await fs.readFile(path.join(output, name))).digest("hex")}  ${name}`);
  await fs.writeFile(path.join(output, "SHA256SUMS.txt"), lines.join("\n") + "\n");
}

async function boundedJson(response) {
  if (!response.ok) throw new Error(`Store request failed (HTTP ${response.status}); credentials and response body omitted`);
  const chunks = []; let length = 0;
  for await (const chunk of response.body) { length += chunk.length; if (length > 128 * 1024) throw new Error("Store response too large"); chunks.push(chunk); }
  return JSON.parse(Buffer.concat(chunks).toString());
}

/** Upload a draft only. There is deliberately no publish/submit endpoint here. */
export async function uploadChromiumDraft({ zip, publisher, item, credentials }, request = fetch) {
  if (!/^[A-Za-z0-9_-]+$/.test(publisher) || !/^[a-p]{32}$/.test(item)) throw new Error("Configure a valid Chrome publisher ID and item ID");
  if (!credentials.clientId || !credentials.clientSecret || !credentials.refreshToken) throw new Error("Chrome OAuth CI secrets are required");
  const stat = await fs.stat(zip);
  if (!stat.isFile() || stat.size > 2 * 1024 * 1024) throw new Error("Unexpected extension package size");
  const token = await boundedJson(await request("https://oauth2.googleapis.com/token", { method: "POST", redirect: "error", signal: AbortSignal.timeout(30000), body: new URLSearchParams({ client_id: credentials.clientId, client_secret: credentials.clientSecret, refresh_token: credentials.refreshToken, grant_type: "refresh_token" }) }));
  if (typeof token.access_token !== "string") throw new Error("Chrome OAuth response did not contain an access token");
  const resource = `publishers/${publisher}/items/${item}`;
  const authorization = `Bearer ${token.access_token}`;
  let result = await boundedJson(await request(`https://chromewebstore.googleapis.com/upload/v2/${resource}:upload?uploadType=media`, { method: "POST", redirect: "error", signal: AbortSignal.timeout(60000), headers: { Authorization: authorization, "Content-Type": "application/zip" }, body: await fs.readFile(zip) }));
  const until = Date.now() + 300000;
  while (result.uploadState === "IN_PROGRESS" && Date.now() < until) {
    await new Promise((resolve) => setTimeout(resolve, 5000));
    const status = await boundedJson(await request(`https://chromewebstore.googleapis.com/v2/${resource}:fetchStatus`, { redirect: "error", signal: AbortSignal.timeout(30000), headers: { Authorization: authorization } }));
    result = { uploadState: status.lastAsyncUploadState };
  }
  if (result.uploadState !== "SUCCEEDED") throw new Error("Chrome draft upload did not succeed; review the developer dashboard");
  return { uploaded_draft: true, published: false };
}

function requireCiApproval() {
  if (process.env.GITHUB_ACTIONS !== "true" || process.env.OSDM_ALLOW_EXTENSION_SIGNING !== "1") throw new Error("Signing/draft upload requires the explicitly approved CI workflow; keep credentials out of local arguments and chat");
}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  const options = Object.fromEntries(args.reduce((pairs, value, i) => i % 2 ? pairs : [...pairs, [value.replace(/^--/, ""), args[i + 1]]], []));
  const output = options.output && path.resolve(options.output);
  const version = validateVersion(options.version ?? "");
  if (!output) throw new Error("--output must be an explicit artifact directory");
  if (command === "build") { await buildPackages(output, version, process.env.OSDM_CHROMIUM_PUBLIC_KEY); return; }
  requireCiApproval();
  if (command === "sign-firefox") {
    if (!process.env.AMO_API_KEY || !process.env.AMO_API_SECRET) throw new Error("Configure AMO_API_KEY and AMO_API_SECRET in the protected CI environment");
    const directory = path.join(output, "source/firefox");
    const manifest = JSON.parse(await fs.readFile(path.join(directory, "manifest.json"), "utf8"));
    if (manifest.version !== version || manifest.browser_specific_settings?.gecko?.id !== "osdm@osdm.app") throw new Error("Firefox staged identity/version mismatch");
    await webExt.cmd.sign({ sourceDir: directory, artifactsDir: output, apiKey: process.env.AMO_API_KEY, apiSecret: process.env.AMO_API_SECRET, channel: "unlisted", amoBaseUrl: "https://addons.mozilla.org/api/v5/", userAgentString: "OSDM explicit CI extension signing" }, { shouldExitProgram: false });
    if (!(await fs.readdir(output)).some((name) => name.endsWith(".xpi"))) throw new Error("Mozilla signing did not produce a signed XPI; this is not a completed signing result");
    await writeChecksums(output);
  } else if (command === "upload-chromium-draft") {
    const publicKey = process.env.OSDM_CHROMIUM_PUBLIC_KEY;
    if (!publicKey || chromiumId(publicKey) !== process.env.CHROME_ITEM_ID) throw new Error("The Chrome public key must match the existing store item ID");
    const result = await uploadChromiumDraft({ zip: path.join(output, `osdm-capture-chromium-${version}.zip`), publisher: process.env.CHROME_PUBLISHER_ID ?? "", item: process.env.CHROME_ITEM_ID ?? "", credentials: { clientId: process.env.CHROME_CLIENT_ID, clientSecret: process.env.CHROME_CLIENT_SECRET, refreshToken: process.env.CHROME_REFRESH_TOKEN } });
    await fs.writeFile(path.join(output, "chromium-draft.json"), JSON.stringify({ version, ...result }, null, 2));
  } else throw new Error("Expected build, sign-firefox, or upload-chromium-draft");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  void main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
