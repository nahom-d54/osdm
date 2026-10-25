/** No network access or credentials: shared input validation for desktop capture. */
export function captureUrls(text: string): string[] {
  const urls = new Set<string>();
  for (const line of text.replace(/^\uFEFF/, "").split(/\r?\n/)) {
    const value = line.trim();
    if (!value || value.startsWith("#")) continue;
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password
    ) {
      throw new Error(
        "Use HTTP(S) links without embedded credentials, one per line.",
      );
    }
    url.hash = "";
    urls.add(url.href);
    if (urls.size > 1000)
      throw new Error("Import at most 1,000 links at a time.");
  }
  return [...urls];
}

export function captureFilename(source: string): string {
  const path = new URL(source).pathname;
  const name = decodeURIComponent(path.slice(path.lastIndexOf("/") + 1));
  if (
    !name ||
    /[<>:"/\\|?*\u0000-\u001f]/.test(name) ||
    /[. ]$/.test(name) ||
    /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(name)
  ) {
    throw new Error(
      "A link has no safe filename. Add that link individually and choose its filename.",
    );
  }
  return name;
}
