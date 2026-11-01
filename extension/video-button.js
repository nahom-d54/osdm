// Isolated-world UI only. Never patch playback, fetch, MediaSource or page JS.
(() => {
  if (globalThis.__osdmVideoButtons) return;
  globalThis.__osdmVideoButtons = true;
  const api = globalThis.browser ?? chrome;
  let settings = { floatingVideos: true, mediaEnabled: true, mediaDisabledSites: [] };
  let host, root, timer, selectedVideo;
  let sourceSignature = "", sourceObject = null, scanSequence = 0, preferredVideo = null;
  let closedVideo = null, manualPosition = false;
  const allowed = () => settings.floatingVideos && settings.mediaEnabled && !settings.mediaDisabledSites.includes(location.hostname);
  const clamp = (value, min, max) => Math.min(Math.max(min, value), Math.max(min, max));

  function close() {
    scanSequence++;
    host?.remove();
    host = root = selectedVideo = null;
    sourceSignature = ""; sourceObject = null;
  }
  function resetBody() {
    const body = root?.querySelector(".body");
    const count = root?.querySelector(".count");
    if (count) count.textContent = "";
    if (body) body.replaceChildren(primaryButton());
  }
  function sourceOf(video) {
    if (!video) return "";
    const sources = [(video.currentSrc || "").slice(0, 8193), (video.src || "").slice(0, 8193)];
    let visited = 0;
    for (const child of video.children) { if (visited++ >= 64) break; if (child.localName === "source") sources.push((child.src || "").slice(0, 8193)); }
    return sources.join("\n");
  }
  const sameSource = (video, signature, object) => video === selectedVideo && video?.isConnected && !video.mediaKeys && sourceOf(video) === signature && video.srcObject === object;
  function button(text, action) {
    const control = document.createElement("button");
    control.textContent = text; control.type = "button";
    control.addEventListener("click", (event) => {
      if (!event.isTrusted || control.disabled) return;
      event.preventDefault(); event.stopPropagation();
      control.disabled = true;
      void action().catch(() => { if (control.isConnected) showMessage("Could not connect. Open the extension and check OSDM."); })
        .finally(() => { control.disabled = false; });
    });
    return control;
  }
  function primaryButton() {
    const control = button("↓ Download with OSDM", scan);
    control.className = "primary";
    return control;
  }
  function showMessage(message) {
    if (!root) return;
    const status = root.querySelector("p");
    status.textContent = message;
  }
  async function scan() {
    const video = selectedVideo, signature = sourceOf(video), object = video?.srcObject;
    const sequence = ++scanSequence;
    root?.querySelector(".body").replaceChildren();
    showMessage("Finding sources for this frame…");
    let reply;
    try { reply = await api.runtime.sendMessage({ op: "video_offers" }); }
    catch { if (sequence === scanSequence) showMessage("Could not scan. Reload the page if the extension was updated, then try again."); return; }
    if (sequence !== scanSequence) return;
    if (!sameSource(video, signature, object)) { showMessage("The video changed. Click Download again to rescan."); return; }
    if (!reply?.ok) { showMessage("Enable media detection and check site permissions in the extension."); return; }
    const body = root?.querySelector(".body"); if (!body) return;
    body.replaceChildren();
    if (!reply.offers.length) { showMessage("No HTTP source found in this frame. Try Video → Scan current page for parent-frame sources. DRM and local-only video cannot be downloaded here."); return; }
    if (reply.offers.length > 1) {
      const count = root.querySelector(".count");
      if (count) count.textContent = `${reply.offers.length} sources`;
    }
    const label = document.createElement("span");
    label.className = "label"; label.textContent = "Source";
    const select = document.createElement("select"); select.setAttribute("aria-label", "Video source to review");
    for (const offer of reply.offers) {
      const url = new URL(offer.url);
      let name = url.pathname.split("/").pop() || "source";
      try { name = decodeURIComponent(name); } catch { /* Display the encoded name. */ }
      name = name.replace(/[\u0000-\u001f\u007f]/g, "").slice(0, 64);
      select.add(new Option(`${offer.kind} · ${url.hostname} · ${name}`, String(select.length)));
    }
    const subtitle = document.createElement("select"); subtitle.setAttribute("aria-label", "Video subtitle to include");
    const tracks = () => {
      subtitle.replaceChildren(new Option("No subtitle", ""));
      for (const [index, track] of (reply.offers[Number(select.value)]?.subtitles || []).entries()) subtitle.add(new Option(track.label, String(index)));
      subtitle.hidden = subtitle.options.length === 1;
    };
    select.addEventListener("change", tracks); tracks();
    body.append(label, select, subtitle, button("Review in OSDM ↗", async () => {
      if (!sameSource(video, signature, object)) { resetBody(); showMessage("The video changed. Click Download again to rescan."); return; }
      const message = { op: "offer_video", token: reply.token, index: Number(select.value), ...(subtitle.value !== "" ? { subtitleIndex: Number(subtitle.value) } : {}) };
      body.replaceChildren(); // The background consumes a selection token once.
      let result;
      try { result = await api.runtime.sendMessage(message); }
      catch { if (sequence === scanSequence) showMessage("Could not connect. Click Download again to rescan before retrying."); return; }
      if (sequence !== scanSequence) return;
      showMessage(result?.ok ? "Sent. Choose quality and destination in OSDM." : "Selection expired or OSDM is unavailable. Click Download again.");
    }));
    showMessage("Choose the source for this video. Browser playback stays unchanged.");
  }

  function startDrag(event) {
    if (event.button !== 0 || event.target.closest("button")) return;
    const box = host.getBoundingClientRect();
    const drag = { dx: event.clientX - box.left, dy: event.clientY - box.top };
    const bar = event.currentTarget;
    bar.setPointerCapture(event.pointerId);
    bar.dataset.dragging = "true";
    event.preventDefault();
    const move = (moveEvent) => {
      if (bar.dataset.dragging !== "true" || !host) return;
      manualPosition = true;
      const current = host.getBoundingClientRect();
      host.style.left = `${clamp(moveEvent.clientX - drag.dx, 4, innerWidth - current.width - 4)}px`;
      host.style.top = `${clamp(moveEvent.clientY - drag.dy, 4, innerHeight - current.height - 4)}px`;
    };
    const end = (endEvent) => {
      delete bar.dataset.dragging;
      bar.releasePointerCapture?.(endEvent.pointerId);
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", end);
      bar.removeEventListener("pointercancel", end);
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", end);
    bar.addEventListener("pointercancel", end);
  }

  function createPanel() {
    host = document.createElement("osdm-video-control");
    host.style.cssText = "all:initial;position:fixed;z-index:2147483647;display:block;width:max-content;max-width:calc(100vw - 8px);left:8px;top:8px";
    root = host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = `:host{color-scheme:dark}*{box-sizing:border-box}
section{width:302px;max-width:calc(100vw - 8px);overflow:hidden;border:1px solid #313b66;border-radius:10px;background:#0f1319;font:12px/1.5 "IBM Plex Sans",system-ui,sans-serif;color:#e9e8e2;box-shadow:0 8px 28px #0007}
.bar{display:flex;align-items:center;gap:7px;padding:7px 8px 7px 6px;background:#161c26;border-bottom:1px solid #242c3a;cursor:grab;touch-action:none;user-select:none}
.bar[data-dragging=true]{cursor:grabbing}
.grip{display:grid;gap:2px;width:10px;padding:4px 2px;border:0;background:transparent;color:#6f7a8f;cursor:grab}
.grip:before{content:"";width:10px;height:8px;background:repeating-linear-gradient(90deg,#6f7a8f 0 1px,transparent 1px 4px),repeating-linear-gradient(0deg,#6f7a8f 0 1px,transparent 1px 4px);opacity:.75}
.brand{font:600 10px "IBM Plex Mono",monospace;letter-spacing:.14em;color:#b9c1d6}
.count{margin-left:2px;color:#8f9ab1;font-size:10px}
.close{margin-left:auto;width:22px;height:22px;padding:0;border:0;border-radius:5px;background:transparent;color:#9aa4b8;font-size:15px;line-height:1;cursor:pointer}
.close:hover{background:#242c3a;color:#e9e8e2}
.body{display:grid;gap:6px;padding:8px}
.label{font:9px "IBM Plex Mono",monospace;letter-spacing:.12em;color:#8f9ab1;text-transform:uppercase}
button,select{font:inherit;border-radius:6px;padding:8px 10px;cursor:pointer}
button{border:0;background:#929dff;color:#0b0d18;font-weight:600}
button:hover:not(:disabled){filter:brightness(1.06)}
button:disabled{opacity:.55;cursor:not-allowed}
select{width:100%;background:#171d27;color:#e9e8e2;border:1px solid #2a3342}
button:focus-visible,select:focus-visible,.grip:focus-visible,.close:focus-visible{outline:2px solid #cfd4ff;outline-offset:2px}
p{margin:0;padding:0 10px 9px;color:#a8b1c4;font-size:11px;overflow-wrap:anywhere}
p:empty{display:none}
@media (prefers-reduced-motion:no-preference){section{animation:osdm-in .12s ease-out}@keyframes osdm-in{from{opacity:.6;transform:translateY(2px)}to{opacity:1;transform:none}}}`;
    const panel = document.createElement("section");
    panel.setAttribute("aria-label", "OSDM video download");
    const bar = document.createElement("header");
    bar.className = "bar";
    const grip = document.createElement("span");
    grip.className = "grip"; grip.setAttribute("role", "button"); grip.tabIndex = 0;
    grip.setAttribute("aria-label", "Move video download panel");
    grip.addEventListener("keydown", (event) => {
      const step = event.shiftKey ? 2 : 12;
      const moves = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
      if (!moves[event.key]) return;
      event.preventDefault();
      manualPosition = true;
      const box = host.getBoundingClientRect();
      host.style.left = `${clamp(box.left + moves[event.key][0], 4, innerWidth - box.width - 4)}px`;
      host.style.top = `${clamp(box.top + moves[event.key][1], 4, innerHeight - box.height - 4)}px`;
    });
    const brand = document.createElement("span");
    brand.className = "brand"; brand.textContent = "OSDM";
    const count = document.createElement("span");
    count.className = "count"; count.setAttribute("aria-live", "polite");
    const dismiss = document.createElement("button");
    dismiss.className = "close"; dismiss.type = "button"; dismiss.textContent = "×";
    dismiss.setAttribute("aria-label", "Close video download panel");
    dismiss.addEventListener("click", () => {
      closedVideo = selectedVideo;
      close();
    });
    bar.append(grip, brand, count, dismiss);
    bar.addEventListener("pointerdown", startDrag);
    const body = document.createElement("div");
    body.className = "body";
    const status = document.createElement("p");
    status.setAttribute("role", "status");
    panel.append(bar, body, status);
    root.append(style, panel);
    document.documentElement.append(host);
    body.append(primaryButton());
    return { bar, body };
  }

  function update() {
    timer = null;
    if (preferredVideo && !preferredVideo.isConnected) preferredVideo = null;
    if (!allowed()) { closedVideo = null; close(); return; }
    const videos = [];
    const roots = [document.documentElement];
    let visited = 0;
    for (let index = 0; index < roots.length && visited < 4000 && videos.length < 64; index++) {
      const walker = document.createTreeWalker(roots[index], 1);
      let node = walker.currentNode;
      while (node && visited++ < 4000 && videos.length < 64) {
        if (node.localName === "video") videos.push(node);
        if (node.shadowRoot && node !== host && roots.length < 32) roots.push(node.shadowRoot);
        node = walker.nextNode();
      }
    }
    const visible = videos.slice(0, 64).filter((video) => {
      const box = video.getBoundingClientRect();
      return (!video.paused || (video === selectedVideo && root?.querySelector("select"))) && !video.ended && !video.mediaKeys && box.width >= 160 && box.height >= 90 && box.bottom > 30 && box.top < innerHeight && box.right > 40 && box.left < innerWidth;
    });
    const area = (video) => { const box = video.getBoundingClientRect(); return Math.max(0, Math.min(innerWidth, box.right) - Math.max(0, box.left)) * Math.max(0, Math.min(innerHeight, box.bottom) - Math.max(0, box.top)); };
    const video = visible.includes(preferredVideo) ? preferredVideo : visible.includes(selectedVideo) ? selectedVideo : visible.sort((a, b) => area(b) - area(a))[0];
    if (!video) { close(); return; }
    if (closedVideo === video) { close(); return; }
    closedVideo = null;
    if (!host) { createPanel(); manualPosition = false; }
    const signature = sourceOf(video);
    if (selectedVideo !== video || sourceSignature !== signature || sourceObject !== video.srcObject) {
      scanSequence++; resetBody(); showMessage(""); selectedVideo = video; sourceSignature = signature; sourceObject = video.srcObject;
      manualPosition = false;
    }
    const parent = document.fullscreenElement && document.fullscreenElement.tagName !== "VIDEO" ? document.fullscreenElement : document.documentElement;
    if (host.parentNode !== parent) parent.append(host);
    if (!manualPosition) {
      const box = video.getBoundingClientRect();
      const panel = host.getBoundingClientRect();
      host.style.left = `${clamp(box.left + 10, 4, innerWidth - panel.width - 4)}px`;
      host.style.top = `${clamp(box.top + 10, 4, innerHeight - panel.height - 4)}px`;
    }
  }
  const schedule = () => { if (!timer) timer = setTimeout(update, 100); };
  for (const name of ["play", "pause", "ended", "loadedmetadata", "loadstart", "emptied", "fullscreenchange"]) document.addEventListener(name, schedule, true);
  document.addEventListener("pointerover", (event) => { if (event.isTrusted) { const video = event.composedPath().find((node) => node?.localName === "video"); if (video) { preferredVideo = video; schedule(); } } }, true);
  addEventListener("scroll", schedule, { passive: true }); addEventListener("resize", schedule);
  new MutationObserver(schedule).observe(document.documentElement, { childList: true, subtree: true });
  // Media events inside shadow players are not always composed. A bounded
  // periodic check also follows layout-only player moves without page hooks.
  setInterval(() => { if (!document.hidden) schedule(); }, 1000);
  api.storage.onChanged.addListener((changes, area) => { if (area === "local") { for (const key of Object.keys(settings)) if (changes[key]) settings[key] = changes[key].newValue; schedule(); } });
  void api.storage.local.get(settings).then((value) => { settings = value; schedule(); });
})();
