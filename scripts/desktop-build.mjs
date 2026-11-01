import fs from "node:fs/promises";
import { constants } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { stageExtension, chromiumId } from "../extension/release.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));

/** Fresh staging only: copy allowlisted extension assets, never whole source trees. */
export async function stageBrowserResources(helper, directory) {
  const name = path.basename(helper);
  if (!["osdm-native-host", "osdm-native-host.exe"].includes(name) || !(await fs.lstat(helper)).isFile()) throw new Error("Expected the built native messaging helper");
  const publicKey = process.env.OSDM_CHROMIUM_PUBLIC_KEY;
  const edgeKey = process.env.OSDM_EDGE_PUBLIC_KEY;
  if (process.env.OSDM_CHROME_EXTENSION_ID && !publicKey) throw new Error("A configured Chrome store ID requires OSDM_CHROMIUM_PUBLIC_KEY for matching unpacked identity");
  if (process.env.OSDM_EDGE_EXTENSION_ID && !edgeKey) throw new Error("A configured Edge store ID requires OSDM_EDGE_PUBLIC_KEY for matching unpacked identity");
  if (publicKey && process.env.OSDM_CHROME_EXTENSION_ID && chromiumId(publicKey) !== process.env.OSDM_CHROME_EXTENSION_ID) throw new Error("Chrome extension public key must match the configured store ID");
  if (edgeKey && process.env.OSDM_EDGE_EXTENSION_ID && chromiumId(edgeKey) !== process.env.OSDM_EDGE_EXTENSION_ID) throw new Error("Edge extension public key must match the configured store ID");
  await fs.mkdir(directory);
  const stagedHelper = path.join(directory, name);
  await fs.copyFile(helper, stagedHelper, constants.COPYFILE_EXCL);
  const extension = path.join(directory, "extension"); await fs.mkdir(extension);
  for (const browser of ["chromium", "firefox"]) {
    const manifest = JSON.parse(await fs.readFile(path.join(root, "extension", `manifest.${browser}.json`), "utf8"));
    await stageExtension(browser, manifest.version, path.join(extension, browser), publicKey);
  }
  const config = { bundle: { resources: {
    // These resources are shipped, not auto-registered or installed in browsers.
    [stagedHelper]: `browser-integration/${name}`,
    [extension]: "browser-integration/extension",
  } } };
  const edgeManifest = JSON.parse(await fs.readFile(path.join(root, "extension", "manifest.chromium.json"), "utf8"));
  await stageExtension("chromium", edgeManifest.version, path.join(extension, "edge"), edgeKey);
  const configPath = path.join(directory, "bundle.json");
  await fs.writeFile(configPath, JSON.stringify(config, null, 2) + "\n", { flag: "wx" });
  return configPath;
}

function command(executable, args, capture = false) {
  const result = spawnSync(executable, args, { cwd: root, encoding: "utf8", stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit", maxBuffer: 8 * 1024 * 1024 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} failed (${result.status ?? "interrupted"})`);
  return result.stdout;
}

async function main() {
  const args = process.argv.slice(2);
  const targetIndex = args.indexOf("--target");
  const inlineTarget = args.find((arg) => arg.startsWith("--target="))?.slice(9);
  const target = inlineTarget ?? (targetIndex < 0 ? null : args[targetIndex + 1]);
  if (targetIndex >= 0 && (!target || target.startsWith("-"))) throw new Error("--target requires a Rust target triple");
  const windows = target ? target.includes("windows") : process.platform === "win32";
  const debug = args.includes("--debug");
  command("cargo", ["build", "--locked", "-p", "osdm-browser-bridge", ...(debug ? [] : ["--release"]), ...(target ? ["--target", target] : [])]);
  const metadata = JSON.parse(command("cargo", ["metadata", "--no-deps", "--format-version", "1"], true));
  const helper = path.join(metadata.target_directory, ...(target ? [target] : []), debug ? "debug" : "release", windows ? "osdm-native-host.exe" : "osdm-native-host");
  const parent = path.join(root, "src-tauri", ".bundle-staging"); await fs.mkdir(parent, { recursive: true });
  // Keep uniquely named staging for inspection; never sweep user/build trees.
  const directory = await fs.mkdtemp(path.join(parent, "browser-"));
  const config = await stageBrowserResources(helper, path.join(directory, "resources"));
  // Invoke the installed Node CLI directly; never interpolate shell arguments.
  const tauri = path.join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
  command(process.execPath, [tauri, "build", ...args, "--config", config]);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
