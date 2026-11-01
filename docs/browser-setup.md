# Guided browser integration

Open **Set up browser extension** from onboarding, Settings, or Capture & queues.
Desktop Settings has keyboard-accessible General, Transfers, Browser, Categories,
Desktop, Layout and Updates tabs. Browser setup lives in **Browser**.
**Manage extension installations** opens an add/edit/remove list for that browser's
allowed IDs. Up to 32 distinct Chromium IDs can coexist; an identical ID across
profiles needs only one entry. Saving requires fresh confirmation and replaces
only this browser's owned allowlist; an empty list disconnects its helper. Firefox
retains its fixed single identity. This list is not a browser-profile inventory.
The three steps are explicit user actions, not silent extension installation:

1. **Add the extension.** If a real store identity is configured at build time,
   open its official Chrome Web Store, Edge Add-ons or Mozilla Add-ons listing and
   approve installation in the browser. With no configured listing, the app says
   so and offers packaged extension files and manual loading instructions instead.
   Firefox temporary loading ends at browser restart; permanent installation needs
   a Mozilla-signed extension. This work does not publish or sign an extension.
2. **Connect the local helper.** The Windows installer offers to configure Chrome,
   Edge, Brave, Chromium and Firefox helpers for the current account. If skipped,
   select a browser here and confirm registration. Packaged builds know their
   extension IDs; no copying is necessary. Windows
   writes only that browser's current-user native-host value; no elevation, startup
   changes or browser-profile scanning occurs. A foreign/modified registration is
   refused. Valid legacy script registrations are migrated without deleting or
   overwriting their old manifest files.
3. **Enable and check.** Enable local integration, then click **Check connection**
   in the extension popup. The app shows the last successful authenticated native
   helper ping for this app session. It does not infer that a particular browser
   or extension is installed. Automatic interception is a separate opt-in setting.

Missing packaged helpers, unavailable extension files, unsupported platforms and
registration conflicts have explicit recovery instructions. The browser mock
refuses native setup rather than fabricating installation/connection status.
Guided registration currently supports Windows; other systems retain the manual
instructions in [extension/README.md](../extension/README.md).

Native-host registration is per Windows user/browser family, not per browser
profile. Installing the same extension identity in several Chrome profiles does
not require separate helper registrations. The selector covers Chrome, Edge,
Brave, unbranded Chromium and Firefox, not every Chromium derivative. Reconnecting
an owned registration retains up to 32 previously authorized Chromium IDs rather
than disconnecting another profile. There are no wildcard extension origins.

If a source/native test left a registration pointing at an
`osdm-native-browser-*` temporary directory, retain its `registration-backup.json`.
The fixed `tests/native-browser/registration.ps1 -Mode Restore -StateFile <backup>`
restores only values still pointing at that test's manifests. Older test runs
could leave a chain of backups; recover newest to oldest, preserving any newer
user/installer changes. Do not delete unrelated native-host registrations.

## Registration safety and removal

- The helper is a fixed packaged resource, not an executable chosen by the webview.
- Chromium IDs must be exactly 32 lowercase letters a–p; Firefox is fixed to
  `osdm@osdm.app`. Configured store IDs cannot be overridden from the UI.
- Manifests live in the app's per-user `native-hosts/guided/` directory with
  browser/content-hash filenames. A create-new manifest is flushed/synced before
  updating the registry. Retries reuse only identical regular files.
- Existing guided registration values are accepted only when their manifest is in
  that owned directory and matches its content hash/schema/ID allowlist. An old
  absolute helper path with the same native-helper filename is repairable after
  an installation moves. Legacy `native-hosts/chromium.json` and `firefox.json`
  are accepted only with the exact OSDM schema/description and valid identities.
  Registration is rechecked before changing it. Unrelated files and
  keys are never overwritten or recursively removed.
- **Remove helper registration** requires separate confirmation. It deletes only
  the owned browser registration's default value, retaining extension files,
  manifests, other registry values/subkeys and other browsers. Local capture may
  remain enabled for other browsers. NSIS uninstall also removes verified owned
  values pointing to this installation's helper, before deleting app files. An old
  installation's uninstaller leaves registrations for a newer installation alone.
- Isolated native-browser tests cannot register hosts or launch stores/folders,
  and status does not read real native-host registry values in test mode.

## Bundled resources

Use the wrapper for distributable builds:

```powershell
pnpm desktop:build --bundles nsis  # Windows
pnpm desktop:build --bundles deb   # Linux
```

`scripts/desktop-build.mjs` builds the matching native helper, stages it and only
allowlisted extension assets in a fresh ignored directory, then supplies a Tauri
resource-config overlay. Builds never change the registry. Interactive NSIS
installation asks before registering helpers; silent installation/updates only
repair registrations already owned by OSDM, never opt in new browsers. Resources
are installed under `browser-integration/`, including separate Chromium, Edge and
Firefox folders. A debug compile/resource smoke check can use
`pnpm desktop:build --debug --no-bundle`. Raw `pnpm tauri build` bypasses this
resource wrapper; development/source builds explain missing resources honestly.

Fresh staging directories remain under `src-tauri/.bundle-staging/` for inspection;
the wrapper never sweeps existing build directories. Desktop release CI uses the
wrapper. Existing capture-tools archives remain available for manual/CLI setups.

## Published-listing configuration

All of these are **public metadata**, not credentials. Configure only after the
corresponding store identity/listing exists:

| Build environment | Release workflow repository variable | Purpose |
| --- | --- | --- |
| `OSDM_CHROME_EXTENSION_ID` | `CHROME_ITEM_ID` | Official Chrome listing ID and helper allowlist |
| `OSDM_CHROMIUM_PUBLIC_KEY` | `CHROME_PUBLIC_KEY` | Matching SPKI key for Chromium unpacked identity |
| `OSDM_EDGE_EXTENSION_ID` | `EDGE_ITEM_ID` | Official Edge listing ID and helper allowlist |
| `OSDM_EDGE_PUBLIC_KEY` | `EDGE_PUBLIC_KEY` | Matching SPKI key for Edge unpacked identity |
| `OSDM_FIREFOX_ADDON_SLUG` | `FIREFOX_ADDON_SLUG` | Existing Mozilla listing slug; helper ID stays fixed |

When a Chromium-family store ID is supplied, the wrapper requires its public key
and verifies that the derived ID matches. No store URL is accepted from webview
arguments: the shell opens only an official URL derived from build-time metadata.
No listing metadata is configured by default. Mozilla signing and Chrome draft
upload remain the separate explicit extension-release workflow.

### Stable identities and store publication

`extension/identity.json` contains a **public-only unpacked fallback key**. It
keeps packaged developer/preview IDs independent of install directory and browser
profile; it is not a signing credential or a published store identity. Source
`prepare.ps1` uses the same key. Do not rotate it casually. The desktop build
wrapper verifies any store key/ID pair and compiles the exact staged IDs into the
desktop; Edge can have a separate store identity. Merely having a stable fallback
ID never creates a store URL. Existing profiles using old keyless unpacked files
need to reload the newly packaged extension files once.

Public store publication still requires the publisher to:

1. Create/claim Chrome Web Store and Edge Add-ons items; obtain their assigned
   IDs/public keys. Create a listed Mozilla Add-ons item for `osdm@osdm.app`.
2. Build extension candidates using `.github/workflows/extensions.yml`, supply
   the store's required privacy disclosures, screenshots and permission rationale,
   submit them in the publisher dashboards and wait for approval. The existing
   Firefox **unlisted** signing action is not a public AMO listing. Chrome draft
   upload likewise does not submit or publish. Edge submission is dashboard-based.
3. Configure the public repository variables above **after** listings are live.
   Keep OAuth/AMO credentials in the protected CI environment, never in chat or git.
4. Build with `OSDM_REQUIRE_BROWSER_STORES=1`; this requires all configured IDs,
   matching keys and the AMO slug. Store publication is currently deferred; this
   gate is opt-in and does not block manual-install previews. It validates
   configuration, not store review/availability;
   verify each real listing in the browser before releasing.
5. Test fresh install, upgrade, opt-out, silent upgrade, uninstall and installation
   approval in two profiles plus Brave. The store action launches the selected
   browser on Windows rather than silently opening a different default browser.

Without those publisher-owned listings, previews still expose the honest manual
fallback under **Advanced**. No enterprise force-install policy, profile editing
or permission bypass is used to pretend publication is complete.

## Verification boundary

Native unit tests use temporary manifests and an in-memory registry; they do not
modify a real browser installation. Packaging tests assert allowlisted assets,
fresh staging and correct helper resource mapping. UI tests use fake IPC for
consent, browser/ID changes, missing helpers, foreign registrations, store/file
actions, removal and session-wide status, with keyboard/light/dark accessibility.
Real installer upgrade/uninstall and store-mediated browser installation still
need interactive validation; source/unit results are not those release gates.

Recorded in this Windows source pass: seven native setup tests pass without any
real registry changes; TypeScript and 17 frontend unit tests pass; 26 extension/
packaging tests pass. `pnpm desktop:build --debug --no-bundle` successfully builds
the native helper and desktop app with the resource overlay. The first focused
UI run found a light/dark hint-contrast issue on the tinted setup panel; the hint
now uses the readable muted-foreground token. The final full UI suite records
48 passes (single worker, 60-second timeout). The release-version Rust workspace
records 235 passed/3 opt-in ignored; locked workspace Clippy and formatting pass.
No installer/store publication or interactive install is claimed by these checks.

Subsequent installer/identity/capture source pass: 28 desktop Rust unit tests
(including 11 browser-setup cases), desktop Clippy, 17 frontend units, 43 extension/
packaging regressions and all 49 UI tests pass. A debug NSIS installer
build succeeds with the helper resources and hooks. Real-browser EXE/media smoke
verification passes on Chrome 138 with a disposable profile: extensionless EXE
handoff and byte-exact engine publication, embedded blob/JavaScript player's
extensionless HLS discovery, app shutdown/reconnect, and missing-host browser
fallback. All original native-host values were restored and independently verified.
A prior Playwright GUID-filename override was not representative of browser UI
downloads; the harness now uses normal profile download preferences. Initial full
UI trace writes hit a full `D:` drive; preserving those artifacts and directing the
rerun to `C:` produced 49 passes without changing assertions.
Store publication remains deferred; interactive installer lifecycle validation
is separate from successfully compiling the installer.
