export const mediaDefaults = { mediaEnabled: true, mediaDisabledSites: [] };
export const mediaLimits = { entries: 256, offers: 32, url: 8192 };

export function mediaUrl(value, base) {
  if (typeof value !== "string" || !value || value.length > mediaLimits.url) return null;
  try {
    const url = new URL(value, base);
    if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return null;
    url.hash = "";
    return url.href.length <= mediaLimits.url ? url.href : null;
  } catch { return null; }
}

export function mediaSite(url) {
  const safe = mediaUrl(url);
  return safe ? new URL(safe).hostname : null;
}

export function mediaAllowed(url, settings) {
  const site = mediaSite(url);
  return !!site && settings.mediaEnabled !== false &&
    !(settings.mediaDisabledSites || []).includes(site);
}

export function toggleMediaSite(sites, url, disabled) {
  const site = mediaSite(url);
  const next = new Set((Array.isArray(sites) ? sites : []).filter((s) => typeof s === "string"));
  if (site) { if (disabled) next.add(site); else next.delete(site); }
  return [...next];
}

export function mediaTitle(title) {
  return (typeof title === "string" ? title : "")
    .replace(/[<>:"/\\|?*\u0000-\u001f\u007f]/g, " ")
    .replace(/\s+/g, " ").trim().slice(0, 160).replace(/[. ]+$/, "") || "Page media";
}

function kind(url, type = "") {
  const path = new URL(url).pathname.toLowerCase();
  if (/\.m3u8$/.test(path) || /mpegurl/i.test(type)) return "HLS";
  if (/\.mpd$/.test(path) || /dash\+xml/i.test(type)) return "DASH";
  if (/\.(mp4|m4v|webm|mkv|mov|mp3|m4a|aac|ogg|oga|ogv|opus|wav|flac)$/.test(path) || /^(video|audio)\//i.test(type)) return "Direct media";
  return null;
}

export function normalizeMedia(snapshot, base) {
  const entries = Array.isArray(snapshot?.entries) ? snapshot.entries.slice(0, mediaLimits.entries) : [];
  const offers = [];
  const seen = new Set();
  for (const entry of entries) {
    if (!entry || entry.source === "track") continue;
    const url = mediaUrl(entry.url, base);
    if (!url || seen.has(url)) continue;
    const detected = kind(url, typeof entry.type === "string" ? entry.type : "");
    // A real media element may use an extensionless endpoint. Resource timing
    // alone must match a known media suffix (never guess from arbitrary fetches).
    if (!detected && !["video", "audio", "source"].includes(entry.source)) continue;
    seen.add(url);
    const subtitles = [];
    const trackSeen = new Set();
    for (const track of entries) {
      if (track?.source !== "track" || track.group !== entry.group || entry.group == null) continue;
      const trackUrl = mediaUrl(track.url, base);
      if (!trackUrl || trackSeen.has(trackUrl)) continue;
      trackSeen.add(trackUrl);
      subtitles.push({ url: trackUrl, label: mediaTitle(track.label || "Subtitle") });
      if (subtitles.length === mediaLimits.offers) break;
    }
    offers.push({ url, kind: detected || "Direct media", title: mediaTitle(snapshot.title), subtitles });
    if (offers.length === mediaLimits.offers) break;
  }
  return offers;
}

// Serialized by scripting.executeScript: deliberately self-contained, read-only,
// isolated-world code. No page messages, network requests, or playback controls.
export function collectMedia() {
  const entries = [];
  const add = (entry) => {
    if (entries.length < 256 && typeof entry.url === "string" && entry.url.length <= 8192)
      entries.push(entry);
  };
  const walker = document.createTreeWalker(document.documentElement, 1);
  let node = walker.currentNode;
  let visited = 0;
  let group = 0;
  while (node && visited++ < 4000 && entries.length < 192) {
    if (node.localName === "video" || node.localName === "audio") {
      const id = group++;
      const source = node.localName;
      if (!node.mediaKeys) {
        add({ url: node.currentSrc, type: node.type, source, group: id });
        add({ url: node.src, type: node.type, source, group: id });
        let children = 0;
        for (const child of node.children) {
          if (children++ >= 256 || entries.length >= 192) break;
          if (child.localName === "source")
            add({ url: child.src, type: child.type, source: "source", group: id });
          if (child.localName === "track" && ["subtitles", "captions"].includes(child.kind))
            add({ url: child.src, source: "track", group: id, label: child.label || child.srclang });
        }
      }
    }
    node = walker.nextNode();
  }
  // Inspect only the most recent timing entries; rescan after playback starts.
  for (const resource of performance.getEntriesByType("resource").slice(-256)) {
    if (entries.length >= 256) break;
    if (/\.(m3u8|mpd|mp4|m4v|webm|mkv|mov|mp3|m4a|aac|ogg|oga|ogv|opus|wav|flac)(?:[?#]|$)/i.test(resource.name))
      add({ url: resource.name, source: "resource" });
  }
  return { title: document.title.slice(0, 512), entries };
}

export function isMediaPopup(sender, runtime) {
  return sender?.id === runtime.id && !sender.tab && sender.url === runtime.getURL("options.html");
}
