export function eligible(url, referrer, rules, bypass = false) {
  if (!rules.enabled || bypass) return false;
  try {
    const parsed = new URL(url);
    if (
      !["http:", "https:"].includes(parsed.protocol) ||
      parsed.username ||
      parsed.password
    )
      return false;
    const hosts = [parsed.hostname];
    if (referrer) hosts.push(new URL(referrer).hostname);
    const excluded = (rules.exclusions || "")
      .toLowerCase()
      .split(/[\s,]+/)
      .filter(Boolean);
    // Referrer-Policy can hide the initiating page. With exclusions configured,
    // keep the browser downloader rather than guessing that page was allowed.
    if (excluded.length && !referrer) return false;
    if (
      hosts.some((host) =>
        excluded.some((site) => host === site || host.endsWith(`.${site}`)),
      )
    )
      return false;
    const extensions = (rules.extensions || "")
      .toLowerCase()
      .split(/[\s,]+/)
      .filter(Boolean)
      .map((s) => s.replace(/^\./, ""));
    const path = decodeURIComponent(parsed.pathname).toLowerCase();
    return extensions.some((ext) => path.endsWith(`.${ext}`));
  } catch {
    return false;
  }
}

export function normalizedLinks(values) {
  const urls = new Set();
  for (const value of values) {
    try {
      const url = new URL(value);
      if (
        !["http:", "https:"].includes(url.protocol) ||
        url.username ||
        url.password
      )
        continue;
      url.hash = "";
      urls.add(url.href);
      if (urls.size >= 1000) break;
    } catch {
      /* Not a transferable URL. */
    }
  }
  return [...urls];
}
