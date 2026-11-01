import { mediaAllowed, mediaDefaults, mediaSite, toggleMediaSite } from "./media.js";

const api = globalThis.browser ?? chrome;
const enabled = document.getElementById("media-enabled");
const siteDisabled = document.getElementById("media-site-disabled");
const siteLabel = document.getElementById("media-site");
const scanButton = document.getElementById("media-scan");
const offerButton = document.getElementById("media-offer");
const results = document.getElementById("media-results");
const status = document.getElementById("media-status");
let page;
let settings = await api.storage.local.get(mediaDefaults);
let offers = [];
let selected = null;
let subtitleSelect = null;

function clear() {
  offers = [];
  selected = null;
  results.replaceChildren();
  offerButton.disabled = true;
}
function update() {
  enabled.checked = settings.mediaEnabled;
  const site = mediaSite(page);
  siteLabel.textContent = site ? `(${site})` : "(unavailable)";
  siteDisabled.checked = !!site && settings.mediaDisabledSites.includes(site);
  siteDisabled.disabled = !site;
  scanButton.disabled = !mediaAllowed(page, settings);
}
try { [ { url: page } ] = await api.tabs.query({ active: true, currentWindow: true }); }
catch { status.textContent = "Open a normal HTTP(S) page to scan."; }
update();

enabled.onchange = async () => {
  settings.mediaEnabled = enabled.checked;
  clear();
  await api.storage.local.set({ mediaEnabled: settings.mediaEnabled });
  update();
};
siteDisabled.onchange = async () => {
  // Read fresh preferences so a second popup cannot overwrite another site's choice.
  settings = await api.storage.local.get(mediaDefaults);
  settings.mediaDisabledSites = toggleMediaSite(settings.mediaDisabledSites, page, siteDisabled.checked);
  clear();
  await api.storage.local.set({ mediaDisabledSites: settings.mediaDisabledSites });
  update();
};
function failure(error) {
  status.textContent = error === "disabled"
    ? "Media detection is disabled for this page. Browser media is untouched."
    : error === "rescan"
      ? "The selection expired or the page changed. Scan again. Browser media is untouched."
      : "OSDM is missing, disabled, outdated, or this page cannot be scanned. Check the connection and retry. Browser media is untouched.";
}
scanButton.onclick = async () => {
  clear();
  scanButton.disabled = true;
  status.textContent = "Scanning current page…";
  try {
    const reply = await api.runtime.sendMessage({ op: "scan_media" });
    if (!reply?.ok) { failure(reply?.error); return; }
    offers = reply.offers;
    status.textContent = offers.length ? "Choose one real source. OSDM will review the offer before downloading." : "No supported media found. Start playback and scan again; DRM/blob-only or hidden sources may be unsupported.";
    offers.forEach((offer, index) => {
      const label = document.createElement("label");
      const radio = document.createElement("input");
      radio.type = "radio";
      radio.name = "media-choice";
      const url = new URL(offer.url);
      label.append(radio, document.createTextNode(` ${offer.kind}: ${url.hostname}${url.pathname.slice(-100)} — ${offer.title}`));
      radio.onchange = () => {
        selected = index;
        offerButton.disabled = false;
        subtitleSelect?.remove();
        subtitleSelect = document.createElement("select");
        subtitleSelect.setAttribute("aria-label", "Optional subtitle from this player");
        subtitleSelect.add(new Option("No subtitle", ""));
        offer.subtitles.forEach((track, i) => subtitleSelect.add(new Option(track.label, String(i))));
        label.append(subtitleSelect);
      };
      results.append(label);
    });
  } catch { failure(); }
  finally { update(); }
};
offerButton.onclick = async () => {
  if (selected == null) return;
  offerButton.disabled = true;
  const message = { op: "offer_media", index: selected };
  if (subtitleSelect?.value !== "" && subtitleSelect?.value != null)
    message.subtitleIndex = Number(subtitleSelect.value);
  try {
    const reply = await api.runtime.sendMessage(message);
    if (reply?.ok) status.textContent = "Offer sent. Review it in OSDM. Browser playback is untouched.";
    else failure(reply?.error);
  } catch { failure(); }
  finally { clear(); }
};
