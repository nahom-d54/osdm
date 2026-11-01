# OSDM browser capture — source implementation

Chromium Manifest V3 and Firefox Manifest V3 targets. Automatic interception is
**on by default** in both the extension and desktop app unless explicitly disabled.
URL-matching login cookies are included for HTTPS file captures by default, with a
global opt-out and browser installation/update permission approval. No entire cookie store is
exported. This is not a store-published/signed extension;
full real-browser update, permission and cancellation validation remains a
release gate in [P2 validation](../docs/p2-validation.md).

## Windows setup

### Guided desktop setup

Installed builds made with `pnpm desktop:build` include the native helper and
allowlisted extension files. Open **Set up browser extension** from onboarding,
Settings, or Capture & queues. The Windows installer offers per-user helper setup
for Chrome, Edge, Brave, Chromium and Firefox. Choose a browser, approve extension
installation, connect the helper if installer setup was skipped, enable local integration
and click **Check connection** in the extension popup. There is no administrator
elevation or silent extension installation. Foreign registrations are refused;
owned ones can be removed with a separate confirmation or by uninstalling. Packaged
extension IDs are stable and configured automatically; no ID copying is needed.

No published store listing is configured by default, so the UI explains manual
loading instead of opening a placeholder link. See [guided setup and packaging
configuration](../docs/browser-setup.md) for store metadata, exact safety rules,
resource builds and validation limits.

### Source/manual setup

1. Build/open the desktop app. Build helpers with:
   `cargo build --release -p osdm-browser-bridge -p osdm-cli`.
    Release workflows also package these in a separate capture-tools archive.
    The desktop build wrapper bundles the native helper. Building never registers
    hosts; interactive installer setup requires consent and never installs an
    extension silently.
2. Run `pnpm install`, then `./extension/prepare.ps1 -Browser chromium -Output unpacked-0.1.1` and load `extension/unpacked-0.1.1`
   as an unpacked extension from Chrome/Edge's extensions page (developer mode).
   For Firefox use `-Browser firefox -Output unpacked-firefox` and temporarily load
   that folder's manifest through `about:debugging`. Release Firefox installation
   requires Mozilla signing; temporary extensions do not survive browser restart.
   Preparation now compiles the React/Vite popup and requires a fresh output
   directory; do not load the raw source manifest as an extension. Cross-platform
   equivalent: `pnpm extension:build chromium <fresh-output-directory>`.
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

## Explicit page-media offers

### Capture desk UI and floating video control

The extension popup is React 19/Vite with hash-routed **Capture**, **Video**,
**Rules** and **Connect** pages, local light/dark theme choice, the OSDM logo and
the desktop's typography/color tokens. Production bundles contain only local
compiled UI assets; Vite/React source, dev servers and source maps are not shipped.
`pnpm extension:dev` runs the Vite development entry; ordinary browser preview
explains that native extension APIs are unavailable instead of faking a connection.

In **Video**, **Floating video download button** and network discovery default on.
Approve the browser's installation/update site access and reload existing pages.
A bounded isolated-world script places a small control next
to a playing, visible non-DRM video. A real user click opens source selection;
**Review in OSDM** sends one choice, with a one-use 60-second tab/frame-bound token.
It never patches playback or accepts a source URL supplied by a web page. Disabling
the setting removes existing controls and leaves future static scripts inactive. Discovery
can still require **Network media discovery** for blob/embedded players.

The floating panel has a drag header (pointer or arrow keys) with viewport
clamping, a close button that dismisses it for the current player, and a source
dropdown listing every discovered video source for the active frame (subtitle
choice included). The panel re-anchors when the selected player or its source
changes. Controls stay with the selected playing video and follow a real pointer hover
when several players are visible. Switching the video URL/source list or
`srcObject` invalidates pending and displayed selections. A selected caption can
be offered with its source; a consumed selection cannot be submitted twice.
Network results are frame/document-scoped so an embedded ad is not silently offered
as another frame's video. Use **Video → Scan current page** explicitly for sources
fetched by a parent frame. Cached manifests, typed preloads, declared Open Graph/
Twitter media and more direct container suffixes are discoverable; known TS/ISO
segment responses are excluded. These are discovery improvements, not support for
all HLS/DASH variants, authenticated media, live streams, DRM or local-only blobs.

### HTTP-backed blob/JavaScript/frame players

Click **Enable network media discovery**, approve optional site/webRequest access,
reload the page, start playback, then **Scan current page**. This observes actual
response URLs/content types, including extensionless manifests fetched by hidden
JavaScript or an embedded frame. It does not inspect cookies, request headers,
response bodies or page variables. URLs stay in bounded extension-session memory;
offers expire after five minutes and navigation/disable clears the cache. Scans
also inspect frames where the browser permits script access, without changing playback.

A `blob:` URL itself is not handed to the engine. There must be a real supported
HTTP(S) source behind it; wholly local/generated blobs, DRM and browser-restricted
pages remain unsupported. Discovery does not add live/DRM or new streaming formats.

### Automatic file interception

Automatic interception and local desktop integration default **on** for new
preferences; saved disabled choices are retained. Use **Capture browser downloads**
to opt out. The switch saves immediately; **Save rules** applies suffix/exclusion edits.
Use **Check connection**. EXE matching now uses Chrome/Firefox's
reported filename as well as the URL suffix, so `Content-Disposition` downloads
from extensionless endpoints can be captured. Late filename events are handled;
Chrome's filename callback waits for the handoff, avoiding premature finalization
of small files. The popup reports recent handoff failures without recording URLs.

The transport is extension → browser native messaging → packaged native helper →
authenticated random IPv4 loopback endpoint in the app (`127.0.0.1`, not a public
listener). Missing/disabled helpers leave the browser downloader available. Files
are canceled in the browser only after OSDM has persisted a prepared capture.
After handoff OSDM brings its window forward and opens a confirmation dialog.
The accepted entry is durably paused, not downloading. Choose **Start download**,
**Download later**, or **Discard capture**. Closing keeps it paused. A restart
retains accepted entries paused in the queue (cookies are lost on restart); the
session-only dialog list is not replayed automatically. No lease expires while
the user is reviewing an already accepted capture.

Captured file handoffs also replay the browser's non-secret identity: the
request user agent, `Accept`, `Accept-Language` and `Sec-CH-UA*` headers, bounded
to 16 values and five minutes. `Cookie` is sourced only from the approved cookie
lookup, never from the header map. Request bodies, response headers, conditional/
range, transport and context headers (`Referer`, `Origin`, `Sec-Fetch-*`) are
excluded because they can carry signed URLs or become wrong after a redirect.
Captured identity applies to the original request; cookies/authorization are
stripped across redirect origins. If no request was observed, the browser's own
user agent is still used.

**Include login cookies** defaults on for automatic HTTPS file captures. The
manifest requests cookies and all-site access during installation/update, not
approval for each site. Turn this switch off to capture without automatically reading any
cookies. Existing enabled installations may need **Allow automatic cookie access**
after updating. Permission refusal/revocation keeps matching downloads in the
browser rather than silently dropping credentials. Cookie lookup is only for the
exact download URL and known browser/container store; cookies are bounded, never
written to extension storage or SQLite, and the engine strips them across origins.
Unidentified private stores and partitioned contexts fail closed. Sharing is not
applied to HTTP, media-manifest inspection/jobs or to other-origin redirects.
Media inspection/jobs remain unauthenticated; discovering a media URL does not
promise that an authenticated stream can be fetched by the engine.

The default capture list now includes modern application packages, archives,
documents, audio/video, images and fonts. **Rules → Use built-in file types** opts
existing installations into the expanded list without silently replacing custom
rules. Desktop **Settings → Categories → Add missing built-in file types** merges
the expanded built-ins and Images/Fonts categories while retaining custom folders,
names and domain rules.

Reload the updated extension files to use these changes; installing a new desktop
build alone does not update an already loaded unpacked extension. If moving from
old keyless files to the stable-key package changes its ID, remove the obsolete
extension copy rather than enabling automatic capture in two copies.

Open the extension popup on an HTTP(S) page and click **Scan current page**.
Start playback and scan again to inspect newly loaded resources. Choose one
source and, optionally, one subtitle belonging to that same player, then click
**Offer selected media to OSDM**. Review the offer in the desktop app before
downloading. HLS/DASH manifests are offers for desktop inspection, not automatic
playlist downloads. No player is paused, no browser download is canceled, and
no cookies, Authorization headers, or page messages accompany this operation.
Missing, disabled, or outdated native integration reports failure and leaves
browser media untouched.

Popup scanning uses `scripting` with browser-approved site access. Discovery
reads actual video/audio elements, their source/track children, and media-suffixed
resource timing entries. It visits at most 4,000 DOM elements, inspects the most
recent 256 resource entries, returns at most 256 entries and 32 unique offers,
and limits URLs to 8,192 characters. Page titles provide bounded filename hints;
no quality, format variant, or URL is fabricated. Query strings remain intact
for handoff but are not displayed. Candidate URLs live only in extension memory
and expire after five minutes or one handoff; nothing stores or logs them.

**Enable manual media detection** and **Disable media detection on this site**
persist only a boolean and exact site hostnames (not subdomains or source URLs).
These preferences are independent of automatic download capture. Network
discovery and floating controls default on for all browser-permitted HTTP(S)
sites; static all-frame injection and bounded periodic refresh support playing
videos, including open shadow roots. Controls do not auto-download a source.
Global/site switches opt out; browser-internal/restricted pages, closed shadow
roots, DRM and wholly local blobs remain unsupported. Browser site restrictions
still apply. This is discovery, not a DRM or access-control bypass.

The background accepts popup media commands only from its own `options.html`
document (including hash routes) and extension ID; tab-opened extension UI also
requires the browser-reported extension origin. Floating controls use separate
messages and tab/frame-bound one-use tokens; webpage URLs/headers are ignored.
A scan registers candidates
in memory; an offer selects only one registered index, never a caller-supplied
URL. Navigation or extension-worker restart requires a new scan. The native wire
request is `{op:"offer_media",url,title,subtitle_url?}`; success requires
`{ok:true}`, while rejection/disconnection is shown without native error text
(which might contain secrets). The desktop owns offer validation and confirmation.

## Native protocol and failure policy

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
- `prepare {url,filename?,headers?,explicit_credentials?,review?}` → durable paused ID,
  `duplicate` flag. New capture leases expire after 30 seconds. Preparation and
  lease insertion are one transaction; no HTTP transfer or working file starts.
- `commit {id}` atomically consumes a live lease and leaves the transfer paused
  for desktop review. Repeated
  commits do not enqueue twice. `abort {id}` removes only an uncommitted lease's
  nonrunnable entry, never ordinary history or a committed transfer.

The extension sends `review:true` on every prepare. Older desktop builds reject
this unknown field before the browser download is canceled, rather than silently
starting it with the old immediate-start behavior. Update/rebuild both desktop
and extension together for confirmation-based capture.
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
URL suffixes and browser-reported/late Content-Disposition filenames.
`blob:`, `data:`, DRM, POST-body downloads, and
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
