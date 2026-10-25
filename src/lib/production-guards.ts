/**
 * Remove browser affordances that do not belong in the packaged desktop app.
 * Development builds keep the context menu and reload shortcuts for debugging.
 */
export function installProductionGuards(): void {
  if (!import.meta.env.PROD) return;

  window.addEventListener("contextmenu", (event) => event.preventDefault(), { capture: true });

  window.addEventListener(
    "keydown",
    (event) => {
      const key = event.key.toLowerCase();
      const command = event.ctrlKey || event.metaKey;
      const blocked =
        event.key === "F5" ||
        event.key === "F12" ||
        (command && ["r", "p", "s"].includes(key)) ||
        (command && event.shiftKey && key === "i");
      if (blocked) {
        event.preventDefault();
        event.stopPropagation();
      }
    },
    { capture: true },
  );
}
