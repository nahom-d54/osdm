// Isolated-world UI only. Never patch playback, fetch, MediaSource or page JS.
(() => {
  const api = globalThis.browser ?? chrome;
  let settings = { floatingVideos: false, mediaEnabled: true, mediaDisabledSites: [] };
  let host, root, timer, selectedVideo;
  const allowed = () => settings.floatingVideos && settings.mediaEnabled && !settings.mediaDisabledSites.includes(location.hostname);
  function close() { host?.remove(); host = root = selectedVideo = null; }
  function button(text, action) {
    const button = document.createElement("button"); button.textContent = text; button.type = "button";
    button.addEventListener("click", (event) => { if (event.isTrusted) { event.preventDefault(); event.stopPropagation(); void action().catch(() => showMessage("Could not connect. Open the extension and check OSDM.")); } });
    return button;
  }
  function showMessage(message) { if (!root) return; const status = root.querySelector("p"); status.textContent = message; }
  async function scan() {
    const reply = await api.runtime.sendMessage({ op: "video_offers" });
    if (!reply?.ok) { showMessage("Enable media detection and check site permissions in the extension."); return; }
    const body = root?.querySelector(".body"); if (!body) return;
    body.replaceChildren();
    if (!reply.offers.length) { showMessage("No HTTP source found. Enable network discovery, reload and play again."); return; }
    const select = document.createElement("select"); select.setAttribute("aria-label", "Video source to review");
    for (const offer of reply.offers) select.add(new Option(`${offer.kind} · ${new URL(offer.url).hostname}`, String(select.length)));
    body.append(select, button("Review in OSDM ↗", async () => {
      const result = await api.runtime.sendMessage({ op: "offer_video", token: reply.token, index: Number(select.value) });
      showMessage(result?.ok ? "Sent. Choose quality and destination in OSDM." : "Selection expired or OSDM is unavailable. Click Download again.");
    }));
    showMessage("Choose the source for this video. Browser playback stays unchanged.");
  }
  function update() {
    timer = null;
    if (!allowed()) { close(); return; }
    const video = [...document.querySelectorAll("video")].slice(0, 64).find((video) => {
      const box = video.getBoundingClientRect();
      return !video.paused && !video.ended && !video.mediaKeys && box.width >= 160 && box.height >= 90 && box.bottom > 30 && box.top < innerHeight && box.right > 40 && box.left < innerWidth;
    });
    if (!video) { close(); return; }
    if (!host) {
      host = document.createElement("osdm-video-control");
      host.style.cssText = "all:initial;position:fixed;z-index:2147483647;display:block;width:max-content;max-width:calc(100vw - 16px)";
      root = host.attachShadow({ mode: "open" });
      const style = document.createElement("style");
      style.textContent = ":host{color-scheme:dark}*{box-sizing:border-box}section{font:12px/1.5 system-ui,sans-serif;color:#e9e8e2;background:#101318;border:1px solid #353e78;border-radius:9px;padding:6px;max-width:300px;box-shadow:0 4px 22px #0005}button,select{font:inherit;border-radius:5px;padding:7px 10px;cursor:pointer}button{border:0;background:#929dff;color:#0b0d18;font-weight:600}button:focus-visible,select:focus-visible{outline:2px solid #e9e8e2;outline-offset:2px}select{width:100%;background:#171b21;color:#e9e8e2;border:1px solid #272d36;margin-bottom:6px}p{margin:6px 4px 2px;color:#b7bbc4;max-width:270px;font-size:11px}p:empty{display:none}.body:not(:empty){padding:8px 2px 2px}";
      const panel = document.createElement("section"); panel.setAttribute("aria-label", "OSDM video download");
      const body = document.createElement("div"); body.className = "body";
      const status = document.createElement("p"); status.setAttribute("role", "status");
      panel.append(button("↓ Download with OSDM", scan), body, status); root.append(style, panel); document.documentElement.append(host);
    }
    if (selectedVideo !== video) { root.querySelector(".body").replaceChildren(); showMessage(""); selectedVideo = video; }
    const box = video.getBoundingClientRect();
    host.style.top = `${Math.max(8, Math.min(innerHeight - 48, box.top + 10))}px`;
    host.style.right = `${Math.max(8, innerWidth - Math.min(innerWidth, box.right) + 10)}px`;
  }
  const schedule = () => { if (!timer) timer = setTimeout(update, 100); };
  for (const name of ["play", "pause", "ended", "loadedmetadata", "fullscreenchange"]) document.addEventListener(name, schedule, true);
  addEventListener("scroll", schedule, { passive: true }); addEventListener("resize", schedule);
  new MutationObserver(schedule).observe(document.documentElement, { childList: true, subtree: true });
  api.storage.onChanged.addListener((changes, area) => { if (area === "local") { for (const key of Object.keys(settings)) if (changes[key]) settings[key] = changes[key].newValue; schedule(); } });
  void api.storage.local.get(settings).then((value) => { settings = value; schedule(); });
})();
