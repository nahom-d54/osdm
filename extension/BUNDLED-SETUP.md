# OSDM capture tools

These files are compiled browser extensions, not a Node/Vite source checkout.
The desktop installer is the recommended way to set up the app and native helper.
No store publication or silent extension installation is implied.

1. Install/open the OSDM desktop app.
2. Load `browser-integration/extension/chromium` in Chrome/Brave/Chromium, or
   `browser-integration/extension/edge` in Edge, using Developer mode → Load unpacked.
   Firefox can temporarily load `browser-integration/extension/firefox/manifest.json`
   from `about:debugging`; permanent installation needs Mozilla signing.
3. In OSDM, **Settings → Browser → Set up browser extension**, select the browser
   and register its helper. Additional differently identified copies can be added
   using **Manage extension installations**. Same-ID browser profiles share one entry.
4. Enable local browser integration in OSDM. In the extension's **Capture** page,
   enable capture and approve browser cookie access, or turn cookie sharing off.
   **Connect** checks the actual native connection.
5. **Video** contains manual discovery and optional floating buttons. **Rules**
   controls file suffixes/exclusions. No cookies or native permissions are bypassed.

Advanced Windows users can use `register-host.ps1` with an absolute path to
`osdm-native-host.exe` and the installed extension's actual 32-letter ID. The
desktop setup is preferable because it checks ownership and preserves other IDs.
Keep the helper at its registered path. Do not execute the staged `bundle.json`;
it is build metadata, not installation instructions.

On Linux, use the browser's per-user NativeMessagingHosts directory and a manifest
with `name: app.osdm.capture`, `type: stdio`, an absolute native-helper `path`, and
`allowed_origins: ["chrome-extension://EXTENSION_ID/"]` or Firefox's
`allowed_extensions: ["osdm@osdm.app"]`. Linux registration remains manual.
