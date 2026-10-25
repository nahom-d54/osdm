# osdm-browser-bridge

Per-user, bounded native-message transport for the OSDM browser extension.
No download logic or engine/database ownership. `osdm-native-host` accepts
length-prefixed JSON on stdin and writes only framed JSON responses to stdout.
It reads the per-user endpoint descriptor on every request, disables proxies and
redirects, and forwards only to `127.0.0.1` with a 15-second timeout. HTTP payloads
and acknowledgements use bounded XChaCha20-Poly1305 authenticated envelopes;
request/response directions are cryptographically distinct. The shared key is
never sent as a bearer header, so an impersonator on a stale port cannot read
captured cookies or forge a success response.

Windows descriptors use current-user DPAPI; Unix descriptors use private file
permissions. No captured URL, cookie, Authorization value, or descriptor token is
logged. Protocol/installation and known limitations: [extension README](../../extension/README.md).

Build: `cargo build -p osdm-browser-bridge --release`.
Test: `cargo test -p osdm-browser-bridge`.
