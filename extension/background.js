import { eligible, normalizedLinks } from "./rules.js";
import { installMediaPanel } from "./media-background.js";
import { cookieOrigin, cookiePermission, readCookieHeaders } from "./cookies.js";
import { captureDefaults } from "./defaults.js";
import {
  canonicalUrl,
  capturedRequestHeaders,
  findRequest,
  rememberRequest,
} from "./request-headers.js";

const api = globalThis.browser ?? chrome;
const host = "app.osdm.capture";
const defaults = captureDefaults;
const native = async (message) => {
  try {
    return await api.runtime.sendNativeMessage(host, message);
  } catch {
    return { ok: false, error: "disconnected" };
  }
};
installMediaPanel(api, native);

// Request identity for captured downloads. In-memory is authoritative for the
// common same-worker path; session storage bridges an MV3 worker restart. Neither
// contains request bodies, response headers or cookie-jar values.
const recentRequests = new Map();
let headerFlush = null;
let headerHydration = null;
const hydrateHeaders = () => headerHydration ??= (async () => {
  try {
    const { requestHeaders } = await api.storage.session.get({ requestHeaders: {} });
    const now = Date.now();
    for (const [url, entry] of Object.entries(requestHeaders || {})) {
      if (entry && now - entry.at <= 5 * 60 * 1000) rememberRequest(recentRequests, url, entry.headers.map(([name, value]) => ({ name, value })), entry.at);
    }
  } catch { /* An unavailable session cache falls back to the browser identity. */ }
})();
const flushHeaders = () => {
  headerFlush = null;
  const value = Object.fromEntries(recentRequests);
  void api.storage.session.set({ requestHeaders: value }).catch(() => {});
};
const rememberHeaders = (details) => {
  if (details.tabId < 0 || !canonicalUrl(details.url)) return;
  rememberRequest(recentRequests, details.url, details.requestHeaders || [], Date.now());
  if (!headerFlush) headerFlush = setTimeout(flushHeaders, 500);
};
const installHeaderObserver = () => {
  if (!api.webRequest?.onSendHeaders) return;
  const filter = { urls: ["http://*/*", "https://*/*"] };
  try {
    api.webRequest.onSendHeaders.addListener(rememberHeaders, filter, ["requestHeaders", "extraHeaders"]);
  } catch {
    try { api.webRequest.onSendHeaders.addListener(rememberHeaders, filter, ["requestHeaders"]); }
    catch { /* The browser may withhold request headers; UA fallback remains. */ }
  }
};
installHeaderObserver();
void hydrateHeaders();
api.runtime.onInstalled.addListener(() => { void hydrateHeaders(); installHeaderObserver(); });

async function capturedHeadersFor(url, rules, item) {
  await hydrateHeaders();
  let cookie = null;
  if (rules.cookiesEnabled !== false && cookieOrigin(url)) {
    // A prior one-off context-menu grant for a single site must not silently turn
    // into automatic cookie sharing after an extension update. The enable gesture
    // requests this global HTTPS permission; the actual lookup remains URL-scoped.
    if (!await api.permissions.contains(cookiePermission("all"))) throw new Error("Automatic cookie permission is missing or was revoked");
    cookie = (await readCookieHeaders(api, url, {
      cookieStoreId: item?.cookieStoreId,
      incognito: item?.incognito,
      tab: item?.tab,
    }))?.cookie ?? null;
  }
  return capturedRequestHeaders(
    cookie,
    findRequest(recentRequests, url) || findRequest(recentRequests, item?.url) || [],
    globalThis.navigator?.userAgent,
  );
}
const notificationIcon =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAABEklEQVR4nO3ayQ0CMRQE0YmFwAiQJCECxstfyoiy1FdP9xMcBnE9nq/3P+eiC9ARgC5ARwC6AB0B6AJ0BKAL0BGALkAHAfh2BBBAAAEEEEAAAQQQQAABjgXIKJ4JEL1n6xMQfWgWQMYd21+ByMMzADLGhwAiJaIAWePDALtlIgCZ41MAdkrtAmSPTwNYLbcDUDE+FWCl5CpA1fh0gNmyKwCV40sAZkrPAlSPLwMYlZ8B6BhfCnA3YgTQNb4c4G7sCk5lv5a3wcip7tb2Onzi+FaAVYSuTu0/iJw0HgEYIXR3wf4fcMJ4FOCUCEAXoCNA9IITjgACCCAABvDrEYAuQEcAugAdAegCdASgC9ARgC5A5wPmNjZoOELPRwAAAABJRU5ErkJggg==";
const notify = () => {
  if (api.action) void api.action.setBadgeText({ text: "!" }).catch(() => {});
  void api.notifications
    .create({
      type: "basic",
      iconUrl: notificationIcon,
      title: "OSDM capture needs attention",
      message:
        "The handoff did not finish. Check OSDM or retry the download in your browser.",
    })
    .catch(() => {});
};

async function installMenus() {
  await api.contextMenus.removeAll();
  for (const [id, title, contexts] of [
    ["link", "Download with OSDM", ["link", "video", "audio"]],
    ["cookies", "Download with OSDM using this site's cookies…", ["link"]],
    [
      "selection",
      "Download selected links with OSDM (up to 1,000)",
      ["selection"],
    ],
    ["page", "Download page links with OSDM (up to 1,000)", ["page"]],
  ])
    api.contextMenus.create({ id, title, contexts });
}
api.runtime.onInstalled.addListener(() => {
  void installMenus();
  // Static scripts cover future documents; also wire already-open ordinary tabs.
  void api.tabs.query({}).then((tabs) => Promise.allSettled(tabs.slice(0, 128).filter((tab) => tab.id != null && /^https?:/.test(tab.url || "")).map((tab) => api.scripting.executeScript({ target: { tabId: tab.id, allFrames: true }, files: ["video-button.js"] })))).catch(() => {});
});

async function prepare(url, headers, browserFilename) {
  const parsed = new URL(url);
  let filename = browserFilename?.split(/[\\/]/).at(-1);
  if (!filename) {
    try { filename = decodeURIComponent(parsed.pathname.split("/").at(-1) || "download"); }
    catch { filename = "download"; }
  }
  return native({
    op: "prepare",
    url,
    filename,
    headers: headers || null,
    explicit_credentials: !!headers,
    review: true,
  });
}

async function captureExplicit(urls, headers) {
  for (const url of normalizedLinks(urls)) {
    const prepared = await prepare(url, headers);
    if (!prepared.ok) {
      notify();
      return;
    }
    if (!prepared.duplicate) {
      const committed = await native({ op: "commit", id: prepared.id });
      if (!committed.ok) {
        notify();
        return;
      }
    }
  }
}

api.contextMenus.onClicked.addListener((info, tab) => {
  // Permission requests must begin inside the user gesture, not after native I/O.
  if (info.menuItemId === "cookies") {
    const url = info.linkUrl;
    if (!cookieOrigin(url)) {
      notify();
      return;
    }
    void api.permissions
      .request(cookiePermission(url))
      .then(async (allowed) => {
        if (!allowed) return;
        await hydrateHeaders();
        const cookie = (await readCookieHeaders(api, url, { tab }))?.cookie ?? null;
        const headers = capturedRequestHeaders(cookie, findRequest(recentRequests, url) || [], globalThis.navigator?.userAgent);
        await captureExplicit([url], headers);
      })
      .catch(notify);
  } else if (info.menuItemId === "link") {
    void capturedHeadersFor(info.linkUrl || info.srcUrl, { cookiesEnabled: false })
      .then((headers) => captureExplicit([info.linkUrl || info.srcUrl], headers))
      .catch(notify);
  } else if (tab?.id != null) {
    void api.scripting
      .executeScript({
        target: { tabId: tab.id },
        func: (selectionOnly) => {
          const selection = window.getSelection();
          const links = [...document.querySelectorAll("a[href]")]
            .filter((a) => !selectionOnly || selection?.containsNode(a, true))
            .map((a) => a.href);
          if (selectionOnly)
            links.push(
              ...(selection?.toString().match(/https?:\/\/[^\s<>"']+/g) || []),
            );
          return links.slice(0, 1000);
        },
        args: [info.menuItemId === "selection"],
      })
      .then((results) => captureExplicit(results[0]?.result || []))
      .catch(notify);
  }
});

api.runtime.onMessage.addListener((message, sender) => {
  if (message?.op === "bypass" && sender.tab) {
    void api.storage.session.set({ bypassUntil: Date.now() + 2500 });
  }
});

const handling = new Map();
const finished = new Set();
const captureStatus = async (outcome) => {
  // No source URL, query, local filename or credentials in diagnostics.
  try { await api.storage.session.set({ lastCapture: { outcome, at: Date.now() } }); } catch { /* Diagnostic only. */ }
};
const handleDownload = (item) => {
  if (handling.has(item.id)) return handling.get(item.id);
  if (finished.has(item.id)) return Promise.resolve();
  const operation = (async () => {
    const rules = await api.storage.local.get(defaults);
    const session = await api.storage.session.get({ bypassUntil: 0 });
    if (
      item.state !== "in_progress" ||
      !eligible(
        item.finalUrl || item.url,
        item.referrer,
        rules,
        Date.now() < session.bypassUntil,
        item.filename,
      )
    )
      return;
    // Only pause after a healthy version handshake. A missing/outdated host never
    // cancels a browser download. Browser-granted cookie access is URL-scoped.
    const health = await native({ op: "ping" });
    if (!health.ok || health.version !== 1) { await captureStatus("disconnected"); return; }
    let headers;
    try { headers = await capturedHeadersFor(item.finalUrl || item.url, rules, item); }
    catch { await captureStatus("cookie_unavailable"); return; }
    let prepared;
    try {
      await api.downloads.pause(item.id);
      prepared = await prepare(item.finalUrl || item.url, headers, item.filename);
      if (!prepared.ok) {
        await api.downloads.resume(item.id);
        await captureStatus("prepare_failed");
        return;
      }
      const current = (await api.downloads.search({ id: item.id }))[0];
      // Firefox represents our own pause as interrupted+paused; Chrome keeps
      // in_progress+paused. Only an actually paused transfer remains ours to
      // hand off. User cancellation/resume must abort the prepared intent.
      if (!current || !current.paused || !["in_progress", "interrupted"].includes(current.state)) {
        if (!prepared.duplicate) await native({ op: "abort", id: prepared.id });
        return;
      }
      await api.downloads.cancel(item.id);
      finished.add(item.id);
      if (finished.size > 256) finished.delete(finished.values().next().value);
      if (!prepared.duplicate) {
        const committed = await native({ op: "commit", id: prepared.id });
        if (!committed.ok) { notify(); await captureStatus("commit_failed"); return; }
      }
      await captureStatus("captured");
    } catch {
      if (prepared?.ok && !prepared.duplicate)
        await native({ op: "abort", id: prepared.id });
      try {
        await api.downloads.resume(item.id);
      } catch {
        /* The user may have canceled it. */
      }
      await captureStatus("handoff_failed");
    }
  })().catch(() => {}).finally(() => handling.delete(item.id));
  handling.set(item.id, operation);
  return operation;
};
api.downloads.onCreated.addListener(handleDownload);
const handleUpdatedDownload = async (item) => {
  // A richer filename event must not be lost behind an earlier URL-only check.
  await handling.get(item.id);
  return handleDownload(item);
};
// Content-Disposition filenames/final redirect URLs can arrive after onCreated.
api.downloads.onChanged?.addListener((delta) => {
  if (!delta.filename && !delta.finalUrl) return;
  return api.downloads.search({ id: delta.id }).then((items) => items[0] && handleUpdatedDownload(items[0])).catch(() => {});
});
// Chrome supplies the server-derived filename here before finalizing the file.
// Keep its filename callback alive while the safe handoff runs, rather than
// allowing a tiny/fast EXE to finish during the native health check.
api.downloads.onDeterminingFilename?.addListener((item, suggest) => {
  void handleUpdatedDownload(item).finally(() => suggest());
  return true;
});
