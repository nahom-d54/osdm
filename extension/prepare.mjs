import fs from "node:fs/promises";
import path from "node:path";
import { stageExtension } from "./release.mjs";
const [browser, output] = process.argv.slice(2);
if (!["chromium", "firefox"].includes(browser) || !output) throw new Error("Use prepare.mjs chromium|firefox <fresh-output-directory>");
const manifest = JSON.parse(await fs.readFile(new URL(`./manifest.${browser}.json`, import.meta.url), "utf8"));
await stageExtension(browser, manifest.version, path.resolve(output), process.env.OSDM_CHROMIUM_PUBLIC_KEY);
