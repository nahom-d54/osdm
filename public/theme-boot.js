// Resolve the stored theme before the first paint so the window never flashes
// the wrong ground. Loaded as a classic script so the Tauri CSP allows it.
(function () {
  try {
    var raw = localStorage.getItem("osdm.settings");
    var stored = raw ? JSON.parse(raw) : null;
    var theme = stored && stored.state && stored.state.settings ? stored.state.settings.theme : "dark";
    var dark =
      theme === "dark" ||
      (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  } catch (error) {
    document.documentElement.dataset.theme = "dark";
  }
})();
