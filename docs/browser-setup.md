# Guided browser integration

Open **Set up browser extension** from onboarding, Settings, or Capture & queues.
The three steps are explicit user actions, not silent extension installation:

1. **Add the extension.** If a real store identity is configured at build time,
   open its official Chrome Web Store, Edge Add-ons or Mozilla Add-ons listing and
   approve installation in the browser. With no configured listing, the app says
   so and offers packaged extension files and manual loading instructions instead.
   Firefox temporary loading ends at browser restart; permanent installation needs
   a Mozilla-signed extension. This work does not publish or sign an extension.
2. **Register the local helper.** Select Chrome, Edge or Firefox, supply the exact
   installed ID when no store ID is configured, and confirm registration. Windows
   writes only that browser's current-user native-host value; no elevation, startup
   changes or browser-profile scanning occurs. A foreign/modified registration is
   refused. Remove legacy script registrations manually before migrating.
3. **Enable and check.** Enable local integration, then click **Check connection**
   in the extension popup. The app shows the last successful authenticated native
   helper ping for this app session. It does not infer that a particular browser
   or extension is installed. Automatic interception is a separate opt-in setting.

Missing packaged helpers, unavailable extension files, unsupported platforms and
registration conflicts have explicit recovery instructions. The browser mock
refuses native setup rather than fabricating installation/connection status.
Guided registration currently supports Windows; other systems retain the manual
instructions in [extension/README.md](../extension/README.md).

## Registration safety and removal

- The helper is a fixed packaged resource, not an executable chosen by the webview.
- Chromium IDs must be exactly 32 lowercase letters a–p; Firefox is fixed to
  `osdm@osdm.app`. Configured store IDs cannot be overridden from the UI.
- Manifests live in the app's per-user `native-hosts/guided/` directory with
  browser/content-hash filenames. A create-new manifest is flushed/synced before
  updating the registry. Retries reuse only identical regular files.
- Existing registration values are accepted only when their manifest is in that
  owned directory, matches its content hash and the expected helper/schema/ID
  allowlist. Registration is rechecked before changing it. Unrelated files and
  keys are never overwritten or recursively removed.
- **Remove helper registration** requires separate confirmation. It deletes only
  the owned browser registration's default value, retaining extension files,
  manifests, other registry values/subkeys and other browsers. Local capture may
  remain enabled for other browsers. Remove registrations before uninstalling if
  desired; installer-driven per-user cleanup is not implemented.
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
resource-config overlay. No registry changes occur during build/install. Resources
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
