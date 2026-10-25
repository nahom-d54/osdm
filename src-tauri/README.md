# OSDM native shell

Tauri 2 hosts the existing frontend and a single persistent Rust engine.

## Run

From the repository root, with Rust, Visual Studio C++ build tools, WebView2,
Node.js, and pnpm installed:

```powershell
pnpm install
pnpm desktop:dev
```

Build an NSIS package with `pnpm desktop:build`. `cargo check -p osdm-desktop`
checks native code without starting a webview. `cargo test -p osdm-desktop`
exercises IPC dispatch with Tauri's mock runtime and native settings persistence.
Linux development additionally needs Tauri's GTK/WebKit/appindicator system
packages; Windows is the initial packaging target.

The main window closes to tray. Use **Quit** in the tray menu to save and stop
transfers before exiting. A second launch shows the existing window.

## Local data

Tauri's `app_data_dir()` for `app.osdm.desktop` contains:

- `downloads.db`: queue, history, durable segments, orphan-partial ledger.
- `downloads.db.lock`: OS locking sidecar, intentionally retained on disk.
- `settings.db`: typed, atomically persisted settings, independent of webview
  localStorage. Only the first launch imports frontend settings.

Do not remove lock sidecars while OSDM is running. Downloads contain full source
URLs, including signed query parameters. Settings do not accept arbitrary keys
or credentials. No telemetry is included.

## Updates

The tray's explicit update action is configuration-gated. For a publisher build,
set `OSDM_UPDATE_ENDPOINT` and `OSDM_UPDATE_PUBKEY` **before compilation**. The
endpoint is a standard, static Tauri manifest served over HTTPS. A textual
Minisign detached signature of the exact JSON bytes must be served at the same
URL with `.minisig` appended. Its key must match the artifact-signing public key.

The public key uses Tauri's base64-encoded public-key file representation. Keep
the private key out of this repository. Use the Tauri signing tooling for the
installer and Minisign-compatible tooling for the raw manifest signature. Set
`bundle.createUpdaterArtifacts` in the publisher's Tauri config overlay when
generating signed release artifacts; the development config does not require a
signing secret or create unsigned updater output.

The shell rejects unsigned/tampered metadata, refuses non-newer versions,
requires explicit consent, verifies the package signature, and drains downloads
before installation. No endpoint or production public key is supplied here.
Live publisher update and installer-upgrade validation remain release gates.

See `docs/bridge-contract.md` for exact command names, casing, and semantics.
