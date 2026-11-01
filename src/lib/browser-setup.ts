export type BrowserTarget = "chrome" | "edge" | "brave" | "chromium" | "firefox";

export interface BrowserSetupStatus {
  registration_supported: boolean;
  helper_available: boolean;
  capture_enabled: boolean;
  last_handshake: number | null;
  browsers: Array<{
    browser: BrowserTarget;
    name: string;
    store_url: string | null;
    extension_id: string | null;
    extension_ids: string[];
    registered: boolean;
    conflict: boolean;
    unpacked_available: boolean;
  }>;
}

export function validExtensionId(browser: BrowserTarget, value: string): boolean {
  return browser === "firefox" ? value === "osdm@osdm.app" : /^[a-p]{32}$/.test(value);
}

export const BROWSER_MANUAL_SETUP: Record<BrowserTarget, string> = {
  chrome: "Open chrome://extensions, enable Developer mode, choose Load unpacked, and select the Chromium folder. Packaged builds configure the extension identity automatically.",
  edge: "Open edge://extensions, enable Developer mode, choose Load unpacked, and select the Edge folder. Packaged builds configure the extension identity automatically.",
  brave: "Open brave://extensions, enable Developer mode, choose Load unpacked, and select the Chromium folder. Packaged builds use a stable extension ID across profiles.",
  chromium: "Open chrome://extensions, enable Developer mode, choose Load unpacked, and select the Chromium folder. Packaged builds use a stable extension ID across profiles.",
  firefox: "Open about:debugging#/runtime/this-firefox, choose Load Temporary Add-on, and select manifest.json in the Firefox folder. Temporary loading lasts only until Firefox restarts; permanent installation requires a Mozilla-signed release.",
};
