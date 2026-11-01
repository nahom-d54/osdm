// Bounded, non-secret request identity. Mirrors download-engine's allowlist;
// credentials, range/conditional and hop-by-hop headers are never replayed.
// Context headers such as Referer/Origin can carry signed URLs and would follow
// a cross-origin redirect; they are deliberately excluded.
export const replayHeaderNames = new Set([
  "accept",
  "accept-language",
  "sec-ch-ua",
  "sec-ch-ua-mobile",
  "sec-ch-ua-platform",
]);
export const requestHeaderLifetime = 5 * 60 * 1000;
export const requestHeaderLimit = 256;

export function canonicalUrl(value) {
  try {
    const url = new URL(value);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return null;
    url.hash = "";
    return url.href;
  } catch { return null; }
}

export function validUserAgent(value) {
  return typeof value === "string" && value.trim() !== "" && value.length <= 512 && !/[\x00-\x1f\x7f-\uffff]/.test(value);
}

/** Returns at most 16 unique, validated pairs. Never includes Cookie. */
export function replayableHeaders(headers) {
  const pairs = [];
  const seen = new Set();
  for (const header of Array.isArray(headers) ? headers.slice(0, 64) : []) {
    const name = typeof header?.name === "string" ? header.name.toLowerCase() : "";
    const value = header?.value;
    if (!replayHeaderNames.has(name) || seen.has(name) || typeof value !== "string" || !value ||
        value.length > 2048 || /[\x00-\x1f\x7f-\uffff]/.test(value)) continue;
    seen.add(name);
    pairs.push([name, value]);
    if (pairs.length >= 16) break;
  }
  return pairs;
}

export function rememberRequest(map, url, headers, now = Date.now()) {
  const key = canonicalUrl(url);
  const pairs = replayableHeaders(headers);
  if (!key || !pairs.length) return map;
  map.delete(key);
  map.set(key, { at: now, headers: pairs });
  while (map.size > requestHeaderLimit) map.delete(map.keys().next().value);
  return map;
}

export function findRequest(map, url, now = Date.now()) {
  const key = canonicalUrl(url);
  if (!key) return null;
  const entry = map.get(key);
  if (!entry || now - entry.at > requestHeaderLifetime) {
    if (entry) map.delete(key);
    return null;
  }
  return entry.headers;
}

/** Capture envelope accepted by the native `prepare` message. */
export function capturedRequestHeaders(cookie, pairs, userAgent) {
  return {
    cookie: typeof cookie === "string" && cookie ? cookie : null,
    authorization: null,
    user_agent: validUserAgent(userAgent) ? userAgent : null,
    headers: (Array.isArray(pairs) ? pairs : []).slice(0, 16).map(([name, value]) => ({ name, value })),
  };
}
