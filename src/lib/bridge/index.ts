import type { EngineBridge } from "@/lib/bridge/types";
import { MockBridge } from "@/lib/bridge/mock";
import { TauriBridge } from "@/lib/bridge/tauri";

let bridge: EngineBridge | null = null;

/**
 * Returns the Tauri bridge when running inside the desktop shell,
 * otherwise the mock bridge so the shell runs in a plain browser tab
 * without an engine. Selection is synchronous: `__TAURI_INTERNALS__`
 * exists only inside the webview provided by src-tauri.
 */
export function getBridge(): EngineBridge {
  if (!bridge) {
    const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
    bridge = inTauri ? new TauriBridge() : new MockBridge();
  }
  return bridge;
}
