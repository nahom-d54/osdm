# OSDM

A Rust + TypeScript desktop download manager, starting with a standalone Rust download engine.

- **[Priority-driven product and engineering plan](PLAN.md)**
- **[Engine documentation](crates/download-engine/README.md)**
- **[Intelligent download phases](docs/intelligent-downloads.md)**

`src/` contains the desktop UI and `src-tauri/` contains its native shell. The
engine supports adaptive connection limits, recoverable ranged transfers, and
optional SHA-256 verification before publication.

## Development

Install a current stable Rust toolchain and the platform C/C++ build tools required by SQLite.

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Try the headless engine with an absolute destination path whose parent already exists:

```powershell
cargo run -p download-engine --example download -- "https://example.com/file.zip" "D:\Downloads\file.zip"
```

The destination must not exist. Downloads publish through hard links, requiring a compatible filesystem such as NTFS. The example stores its queue in `osdm.db` in the current directory.
