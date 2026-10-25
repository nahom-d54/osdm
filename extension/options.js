const api = globalThis.browser ?? chrome;
const defaults = {
  enabled: false,
  exclusions: "",
  extensions: "zip exe msi iso pdf mp4 mkv 7z tar.gz",
};
const status = document.getElementById("status");
const fields = ["enabled", "exclusions", "extensions"];
const values = await api.storage.local.get(defaults);
if ((await api.action.getBadgeText({})) === "!")
  status.textContent =
    "A handoff needs attention: check OSDM's queue or retry in your browser before clearing this warning.";
document.getElementById("clear-warning").onclick = async () => {
  await api.action.setBadgeText({ text: "" });
  status.textContent =
    "Warning cleared. This does not retry or confirm a download.";
};
for (const id of fields) {
  const field = document.getElementById(id);
  if (id === "enabled") field.checked = values[id];
  else field.value = values[id];
}
document.getElementById("save").onclick = async () => {
  await api.storage.local.set(
    Object.fromEntries(
      fields.map((id) => [
        id,
        id === "enabled"
          ? document.getElementById(id).checked
          : document.getElementById(id).value,
      ]),
    ),
  );
  status.textContent = "Rules saved.";
};
document.getElementById("check").onclick = async () => {
  try {
    const reply = await api.runtime.sendNativeMessage("app.osdm.capture", {
      op: "ping",
    });
    status.textContent =
      reply.ok && reply.version === 1
        ? "Connected to OSDM."
        : "Enable integration in OSDM, or update the host and extension.";
  } catch {
    status.textContent =
      "Not connected. Open OSDM and check native-host registration.";
  }
};
document.getElementById("modifiers").onclick = async () => {
  try {
    const allowed = await api.permissions.request({
      origins: ["http://*/*", "https://*/*"],
    });
    if (!allowed) return;
    await api.scripting
      .unregisterContentScripts({ ids: ["osdm-modifiers"] })
      .catch(() => {});
    await api.scripting.registerContentScripts([
      {
        id: "osdm-modifiers",
        matches: ["http://*/*", "https://*/*"],
        js: ["modifiers.js"],
        runAt: "document_start",
        persistAcrossSessions: true,
      },
    ]);
    status.textContent =
      "Hold Shift or Alt while clicking a download to bypass capture. Reload existing tabs first.";
  } catch {
    status.textContent =
      "This browser cannot enable modifier bypass. Disable automatic capture to use its downloader.";
  }
};
