// Installed only after the user opts into site access for modifier-key bypass.
document.addEventListener(
  "mousedown",
  (event) => {
    if (event.shiftKey || event.altKey) {
      const api = globalThis.browser ?? chrome;
      void api.runtime.sendMessage({ op: "bypass" }).catch(() => {});
    }
  },
  true,
);
