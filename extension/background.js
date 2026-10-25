import { eligible, normalizedLinks } from "./rules.js";

const api = globalThis.browser ?? chrome;
const host = "app.osdm.capture";
const defaults = {
  enabled: false,
  exclusions: "",
  extensions: "zip exe msi iso pdf mp4 mkv 7z tar.gz",
};
const native = async (message) => {
  try {
    return await api.runtime.sendNativeMessage(host, message);
  } catch {
    return { ok: false, error: "disconnected" };
  }
};
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
});

async function prepare(url, headers) {
  const parsed = new URL(url);
  const filename = decodeURIComponent(
    parsed.pathname.split("/").at(-1) || "download",
  );
  return native({
    op: "prepare",
    url,
    filename,
    headers: headers || null,
    explicit_credentials: !!headers,
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
    const parsed = new URL(url);
    const origin = `${parsed.protocol}//${parsed.hostname}`;
    if (!url.startsWith("https://")) {
      notify();
      return;
    }
    void api.permissions
      .request({ permissions: ["cookies"], origins: [`${origin}/*`] })
      .then(async (allowed) => {
        if (!allowed) return;
        const cookies = await api.cookies.getAll({ url }); // Only cookies applicable to this one URL.
        await captureExplicit([url], {
          cookie: cookies.map((c) => `${c.name}=${c.value}`).join("; "),
          authorization: null,
          user_agent: null,
        });
      })
      .catch(notify);
  } else if (info.menuItemId === "link") {
    void captureExplicit([info.linkUrl || info.srcUrl]).catch(notify);
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

api.downloads.onCreated.addListener((item) => {
  return (async () => {
    const rules = await api.storage.local.get(defaults);
    const session = await api.storage.session.get({ bypassUntil: 0 });
    if (
      item.state !== "in_progress" ||
      !eligible(
        item.finalUrl || item.url,
        item.referrer,
        rules,
        Date.now() < session.bypassUntil,
      )
    )
      return;
    // Only pause after a healthy version handshake. A missing/outdated host never
    // cancels a browser download. No cookies accompany automatic interception.
    const health = await native({ op: "ping" });
    if (!health.ok || health.version !== 1) return;
    let prepared;
    try {
      await api.downloads.pause(item.id);
      prepared = await prepare(item.finalUrl || item.url);
      if (!prepared.ok) {
        await api.downloads.resume(item.id);
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
      if (!prepared.duplicate) {
        const committed = await native({ op: "commit", id: prepared.id });
        if (!committed.ok) notify();
      }
    } catch {
      if (prepared?.ok && !prepared.duplicate)
        await native({ op: "abort", id: prepared.id });
      try {
        await api.downloads.resume(item.id);
      } catch {
        /* The user may have canceled it. */
      }
    }
  })().catch(() => {});
});
