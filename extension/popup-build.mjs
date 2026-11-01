import { build } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";

let compiled;
export async function stagePopup(directory) {
  compiled ||= build({ configFile: false, root: fileURLToPath(new URL("./", import.meta.url)), base: "./", publicDir: false, plugins: [react()], logLevel: "error",
    build: { write: false, sourcemap: false, cssCodeSplit: false, rollupOptions: { input: fileURLToPath(new URL("./options.html", import.meta.url)), output: { entryFileNames: "ui-assets/popup.js", chunkFileNames: "ui-assets/[name]-[hash].js", assetFileNames: "ui-assets/[name]-[hash][extname]" } } } });
  const result = await compiled;
  for (const entry of (Array.isArray(result) ? result : [result]).flatMap((bundle) => bundle.output)) {
    if (entry.fileName !== "options.html" && !entry.fileName.startsWith("ui-assets/")) throw new Error("Unexpected popup build output");
    const output = path.join(directory, entry.fileName);
    await fs.mkdir(path.dirname(output), { recursive: true });
    await fs.writeFile(output, entry.type === "asset" ? entry.source : entry.code, { flag: "wx" });
  }
}
