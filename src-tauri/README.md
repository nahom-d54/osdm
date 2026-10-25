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

Release builds check `nahom-d54/osdm` on the public GitHub Releases API. The
checker selects the highest semver release newer than the running build, with an
optional prerelease filter, then chooses the platform package
(`*_x64-setup.exe` on Windows, `*_amd64.deb` on Linux). It requires an exact
`SHA256SUMS.txt` entry and cross-checks the API asset digest when present before
offering the update. Downloads require explicit consent.

Automatic checks run shortly after launch and every six hours. They can be
disabled in Settings, where a manual **Check now** action is also available. The
tray's explicit check uses the same path. No update is installed silently.

On Windows the verified installer is launched as
`installer.exe /UPDATE /P /R`. `/UPDATE` keeps the existing installation and
skips the uninstall page, `/P` shows passive progress, and `/R` restarts OSDM
after installation. The engine is shut down and its queue is saved before the
app exits. On Linux the verified Debian package is opened in the system software
installer.

The GitHub path is checksum- and TLS-verified, not publisher-signed. If
`OSDM_UPDATE_ENDPOINT` and `OSDM_UPDATE_PUBKEY` are set **before compilation**,
the shell instead uses the signed Tauri manifest path. That path requires a
standard HTTPS Tauri manifest plus a textual Minisign signature at
`<endpoint>.minisig`, and verifies both the manifest and artifact before
installation. Keep the private key out of the repository.

See `docs/bridge-contract.md` for exact command names, casing, and semantics.
