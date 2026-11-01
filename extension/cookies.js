export function cookieOrigin(value) {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && !url.username && !url.password ? url.origin : null;
  } catch { return null; }
}

export function cookiePermission(value) {
  if (value === "all") return { permissions: ["cookies"], origins: ["https://*/*"] };
  const origin = cookieOrigin(value);
  if (!origin) throw new Error("Choose an HTTPS site without embedded credentials");
  // Browser match patterns cannot restrict ports. URL-matching lookup and the
  // engine still restrict credentials to the exact scheme/host/port origin.
  return { permissions: ["cookies"], origins: [`https://${new URL(origin).hostname}/*`] };
}

export async function readCookieHeaders(api, url, context = {}) {
  if (!cookieOrigin(url)) throw new Error("Cookie capture requires HTTPS");
  let storeId = context.cookieStoreId;
  if (context.tab?.id != null) {
    storeId = context.tab.cookieStoreId;
    if (!storeId) {
      const stores = await api.cookies.getAllCookieStores();
      storeId = stores.find((store) => store.tabIds.includes(context.tab.id))?.id;
    }
    if (!storeId) throw new Error("Cannot identify this tab's cookie store");
  }
  if ((context.incognito || context.tab?.incognito) && !storeId) throw new Error("Cannot identify the private cookie store; use the browser downloader");
  const cookies = await api.cookies.getAll({ url, ...(storeId ? { storeId } : {}) });
  if (cookies.length > 256 || cookies.some((cookie) => cookie.partitionKey || cookie.firstPartyDomain)) throw new Error("This cookie context cannot be replayed safely; use the browser downloader");
  cookies.sort((a, b) => (b.path?.length || 0) - (a.path?.length || 0));
  const pairs = cookies.map(({ name, value }) => {
    if (!/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(name) || typeof value !== "string" || /[\x00-\x20\x7f-\uffff;,]/.test(value)) throw new Error("Unsupported cookie header");
    return `${name}=${value}`;
  });
  const cookie = pairs.join("; ");
  if (cookie.length > 16384) throw new Error("Cookie header exceeds the capture limit");
  return cookie ? { cookie, authorization: null, user_agent: null } : null;
}

export async function automaticCookieHeaders(api, url, rules, item) {
  const origin = cookieOrigin(url);
  if (!origin || rules.cookiesEnabled === false) return null;
  // A prior one-off context-menu grant for a single site must not silently turn
  // into automatic cookie sharing after an extension update. The enable gesture
  // requests this global HTTPS permission; the actual lookup remains URL-scoped.
  if (!await api.permissions.contains(cookiePermission("all"))) throw new Error("Automatic cookie permission is missing or was revoked");
  // Chrome's spanning incognito mode provides no reliable download tab/store
  // identity. Do not guess or read the normal profile's login instead.
  return readCookieHeaders(api, url, { cookieStoreId: item.cookieStoreId, incognito: item.incognito });
}
