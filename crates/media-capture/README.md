# OSDM media capture foundation

P3 work in progress: **inspection only**, not a streaming downloader or browser
capture integration. `inspect_hls(final_response_url, bounded_body)` identifies
quality choices in basic HLS master playlists and ordered segments in complete,
unencrypted VOD playlists. Variant bandwidth is declared metadata, not measured
throughput. Choosing a variant must remain an explicit user action.

Limits: 1 MiB manifest, 8 KiB lines/URL, 4096 entries, UTF-8, HTTP(S) only, no
embedded credentials/fragments or HTTPS-to-HTTP segment references. Relative URLs
resolve against the final manifest URL; parent query strings are not copied into
child URLs. Debug output and errors omit source URLs. Callers must not log the
explicit `MediaUrl::as_str()` value.

The parser intentionally rejects unknown EXT tags/variant attributes, live/event
streams, encryption/DRM, byte ranges, initialization maps, discontinuities,
external audio/subtitle renditions, and newer protocol versions. This conservative
subset avoids claiming usable output when required semantics are unimplemented.
No codecs are decoded, executed, or bundled. This crate makes no network requests
or filesystem writes and does not depend on the transfer engine. The engine now
depends on this parser for its bounded, cancellable `Engine::inspect_hls` API.

Engine inspection uses engine budgets and bounded cancellable fetching, with
explicit redirect-origin authorization and no credential forwarding. Future
downloading still requires durable project/segment identity and checkpointing,
and verified no-clobber final publication. A parsed URL is **not** network
authorization: cross-origin/CDN references need explicit policy before fetching.
Do not concatenate arbitrary segments and call the result verified media.

Run `cargo test -p osdm-media-capture`.
