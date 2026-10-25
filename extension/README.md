# OSDM browser capture — source implementation

Chromium Manifest V3 and Firefox Manifest V3 targets. Automatic interception is
**off by default** in both the extension and desktop app. No browser credential
store is read by ordinary capture. This is not a store-published/signed extension;
full real-browser update, permission and cancellation validation remains a
release gate in [P2 validation](../docs/p2-validation.md).

## Windows setup

1. Build/open the desktop app. Build helpers with:
   `cargo build --release -p osdm-browser-bridge -p osdm-cli`.
   Future release workflows package these in a separate capture-tools archive;
   they are not silently registered by the desktop installer.
2. Run `./extension/prepare.ps1 -Browser chromium` and load `extension/unpacked`
   as an unpacked extension from Chrome/Edge's extensions page (developer mode).
   For Firefox use `-Browser firefox -Output unpacked-firefox` and temporarily load
   that folder's manifest through `about:debugging`. Release Firefox installation
   requires Mozilla signing; temporary extensions do not survive browser restart.
3. Copy the actual Chromium extension ID. Register the native helper explicitly:

   ```powershell
   ./extension/register-host.ps1 -HostExecutable 'C:\path\osdm-native-host.exe' -ChromiumExtensionId 'your-32-letter-extension-id'
   ```

   This writes only current-user Chrome, Edge, and Firefox native-host registration.
   The Chromium ID must be the real 32-letter ID (a–p); Firefox's ID is
   `osdm@osdm.app`. No administrator elevation is needed. Keep the helper at this
   path; if you move it, register again. The manifest allowlists only these IDs.
4. In OSDM open **Capture & queues → Enable local browser integration**. Choose
   the default destination in Settings. The existing OS downloads/home folder is
   the fallback. The extension cannot choose an arbitrary absolute destination.
5. Open the extension popup and **Check connection**. Configure file suffixes,
   exclusions, and optionally enable automatic capture. Context menus support a
   selected link/media source, selected links/text, and all page links (max 1,000).

For another compatible Chromium browser, `register-host.ps1` accepts
`-BrowserExecutable` and an explicit per-user `-BrowserRegistryPath` ending in
`NativeMessagingHosts\app.osdm.capture`. Supply the vendor's documented native-host
key; executable detection does not guess an unknown vendor's registry policy.
`-LaunchBrowser` starts the selected executable without shell interpolation.

To unregister, disable desktop capture and run `unregister-host.ps1` (with the
same custom registry path if used). It removes only keys still pointing at this
registration's manifests, not download data. Remove the extension separately.

## Linux setup

Build `osdm-native-host` and install it at a stable executable path. Create a
native messaging manifest with `name: "app.osdm.capture"`, `type: "stdio"`, an
absolute `path` to that binary, and the allowlist:

- Chromium: `allowed_origins: ["chrome-extension://ACTUAL_EXTENSION_ID/"]` in
  `~/.config/google-chrome/NativeMessagingHosts/app.osdm.capture.json` (use the
  browser's documented per-user native-host directory for other vendors).
- Firefox: `allowed_extensions: ["osdm@osdm.app"]` in
  `~/.mozilla/native-messaging-hosts/app.osdm.capture.json`.

The helper discovers the app through the same XDG data directory as Tauri.
Linux native host registration has not been interactively validated here.

## Protocol and failure policy

The extension uses native messaging (`app.osdm.capture`). A small native helper
loads the current per-user endpoint descriptor **for each message**, then forwards
one bounded request to an IPv4 loopback-only HTTP endpoint. It disables proxies
and redirects and uses a 15-second timeout. Native frames are bounded at 256 KiB.
The endpoint uses a fresh 64-hex-character shared key on each app launch. Requests
and responses use XChaCha20-Poly1305 authenticated encryption with separate
request/response associated data and random nonces; the key is **not** transmitted
in an HTTP header. This prevents a stale-port impersonator from reading a cookie
handoff or forging a successful acknowledgement. Web origins are refused and no
CORS permission is granted. The descriptor is DPAPI encrypted for the current
Windows user; Unix files use mode 0600. The key is never returned to the extension
or webview and is not an origin credential. Plain messages/native frames are
limited to 256 KiB; encrypted envelopes/responses are bounded at 400 KiB.

Messages (protocol version 1; helper encrypts/decrypts the HTTP envelope):

- `ping` → `{ok:true,version:1}` only when integration and engine are ready.
- `status {id}` → state, pending-lease flag, SHA-256 and integrity for one entry;
  no source URL, destination path or captured headers are returned.
- `prepare {url,filename?,headers?,explicit_credentials?}` → durable paused ID,
  `duplicate` flag. New capture leases expire after 30 seconds. Preparation and
  lease insertion are one transaction; no HTTP transfer or working file starts.
- `commit {id}` atomically consumes a live lease and queues the transfer. Repeated
  commits do not enqueue twice. `abort {id}` removes only an uncommitted lease's
  nonrunnable entry, never ordinary history or a committed transfer.
- `queues`, `start_queue {id}`, `stop_queue {id}` support the CLI's authenticated
  `--desktop` saved-queue trigger. They do not accept arbitrary filesystem paths.

Automatic interception checks rules and version before pausing a browser download.
It prepares OSDM, checks the browser download was not canceled, cancels the browser
download, then commits. If preparation/cancellation fails it aborts and attempts
to resume the browser. User-canceled downloads are not committed. Abandoned leases
are cleaned after expiry, including after app restart. Duplicates reuse an exact
normalized URL and destination in retained history; different URLs cannot silently
overwrite a matching name. This is not content-aware duplicate detection.

**Distributed handoff limit:** a disconnect/crash after browser cancellation but
before commit acknowledgement cannot be made atomic across the two applications.
The extension notifies the user to check OSDM/retry rather than silently starting
a second browser transfer. An uncommitted lease expires; a committed transfer
remains durable. This uncertain-handoff path and native notification delivery need
live-browser validation before the capture gate can pass.

Exclusions match the source/referrer host and subdomains. If exclusions are set
but the browser hides the initiating referrer, automatic capture is skipped rather
than guessing the page was allowed. File rules match decoded
URL suffixes; downloads with filenames known only from Content-Disposition are
not automatically classified yet. `blob:`, `data:`, DRM, POST-body downloads, and
media manifests are not direct-URL capture targets. Explicit context-menu capture
does not export request bodies or browser authentication state automatically.

The **use this site's cookies** context action requests cookies/site permission
inside the explicit user gesture, reads only cookies applicable to that requested
HTTPS URL, and hands them to one prepared transfer in memory. Headers are never
logged or persisted; cross-origin redirects strip secrets. Restart requires a new
handoff. There is no bulk cookie export or passive Authorization interception.
Permission for Shift/Alt bypass is separate and optional; after granting site
access reload existing tabs so the tiny modifier observer can run. Otherwise
disable automatic capture to use the browser downloader.

`pnpm test:extension` exercises Chromium/Firefox-shaped adapters, rule behavior,
missing-host fallback, user cancellation, and the prepare/cancel/commit ordering.
These mocked targets are not a substitute for installing both real browsers.

## Disposable Windows browser checks

With explicit permission to temporarily register a native host, run:

```powershell
pnpm test:native-browser -Browser both
```

Requires installed Chrome and Firefox. The harness builds **debug** binaries,
checks their isolation support before launch, and creates fresh profiles, app
data, WebView storage, fixture downloads and DPAPI endpoint data under
`%LOCALAPPDATA%\Temp\opencode\osdm-native-browser-*`. It never copies normal
profiles. Test-only extension copies use loopback fixtures; their smoke code is
not included in release packages. No OS power action is executed. Test mode
blocks power actions and startup registration and omits notification/updater
plugins. Release binaries ignore `OSDM_NATIVE_TEST_DATA`.

The original current-user Chrome/Chromium/Firefox native-host default values
are durably backed up before registration. Cleanup restores only values still
matching the test, preserving concurrent user/installer changes. Profiles and
`results.json` remain locally as evidence. After a forcibly terminated harness,
restore its retained backup with:

```powershell
./tests/native-browser/registration.ps1 -Mode Restore -StateFile 'C:\path\to\test\registration-backup.json'
./tests/native-browser/cleanup.ps1 -Root 'C:\path\to\osdm-native-browser-EXAMPLE'
```

Checks cover explicit prepare/abort/commit, SHA-256 publication, automatic
interception, cross-origin credential stripping, app termination/reconnect, and
missing-executable fallback to browser downloads. Chrome worker startup/reload
is exercised. Firefox uses a temporary add-on reinstalled on each launch; this
does **not** establish signed-add-on persistence across browser restarts.

## Extension signing and draft uploads

`.github/workflows/extensions.yml` is manual-only. It builds allowlisted source
packages and checksums; signing/upload inputs default to **false**. Create the
`extension-signing` GitHub environment with required reviewers before enabling
account access. Configure secrets **there**, never in chat or command arguments:

- Mozilla: `AMO_API_KEY`, `AMO_API_SECRET` for **unlisted** XPI signing.
- Chrome: `CHROME_CLIENT_ID`, `CHROME_CLIENT_SECRET`, `CHROME_REFRESH_TOKEN`.
- Environment variables: `CHROME_PUBLISHER_ID`, `CHROME_ITEM_ID`,
  `CHROME_PUBLIC_KEY` (the store item's base64 DER/SPKI public key; not private).

The Chrome key must derive the configured item ID. Create the store item and
obtain OAuth consent separately. The script only uploads a **draft** and has no
publish/submit call. Mozilla signing sends the package for unlisted validation;
it does not create a public listing. No signing/upload is performed merely by
adding this workflow, and candidates are not automatically attached to releases.

Use a monotonically increasing numeric extension version (one to four components,
each 0–65535), not the app's prerelease tag. Artifacts are retained seven days.
Inspect and test the signed candidate before separately approving distribution.
Firefox now requires 142+ for its declared no-data-collection metadata; Chrome
requires 116+. Store approval and signed Firefox restart testing remain gates.
