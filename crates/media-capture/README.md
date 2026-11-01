# OSDM media capture foundation

P3 work in progress: **offline planning and local remux**, not a streaming
downloader or browser capture integration.

## HLS planning

`inspect_hls(final_response_url, bounded_body)` identifies quality choices in
basic HLS master playlists and ordered segments in complete, unencrypted VOD
playlists. Bandwidth is declared metadata, not measured throughput. Average
bandwidth and frame rate are preserved when declared. Choosing a variant must
remain explicit. `inspect_hls_vod` adds manifest and media-sequence identity.
Masters preserve associated `EXT-X-MEDIA` audio/subtitle declarations on each
variant (group/name/language, URI, default/autoselect/forced flags). In-band audio
is distinguished by an absent URI; subtitles require an external URI. Invalid,
duplicate or missing groups fail. Flags never authorize implicit selection.

Limits: 1 MiB manifest, 8 KiB lines/URL, 4096 entries, UTF-8, HTTP(S) only, no
embedded credentials/fragments or HTTPS-to-HTTP references. Relative URLs resolve
against the final manifest URL; parent query strings are not copied to children.
Debug/errors omit URLs and untrusted textual metadata. Never log
`MediaUrl::as_str()`. Plans implement serde: **serialized plans contain secret
URLs**. `MediaUrl` deserialization revalidates URL safety, but deserialized plans
are not authorization or proof their non-URL semantics passed inspection.

Unknown EXT tags/variant attributes, live/event, encryption/DRM, byte ranges,
initialization maps, discontinuities, closed-caption groups, and protocol versions
above 6 fail closed. Version acceptance does not enable unsupported tags/features.
No codecs are bundled. Parsing performs no
network requests or filesystem writes and does not depend on the engine.

## DASH and subtitles

`inspect_dash(final_response_url, bounded_body)` accepts strict static,
single-period DASH with bounded explicit `SegmentList` addressing, optional
initialization objects, inherited lists, and unambiguous hierarchical `BaseURL`.
Every representation remains an explicit choice, preserving adaptation/period/
representation IDs, bandwidth, dimensions, codecs, language, timing and numbered
segment identity. Supported MIME types are MP4 audio/video, MPEG-TS, VTT and SRT.
Declared period/presentation duration is checked against each explicit list
(a shorter last segment is allowed). Time-only ISO durations up to millisecond
precision are supported. There are no automatic quality or track selections.
Unknown elements/attributes, multiple periods/base alternatives, SegmentTemplate,
SegmentTimeline, byte ranges, xlinks, DTD/custom entities, DRM/ContentProtection
and dynamic/live manifests fail closed. XML depth, element count, URLs, metadata
and aggregate segment count are bounded. Unsupported tracks are errors, not skipped.

`validate_subtitles(SubtitleFormat::{Srt, WebVtt}, body)` validates bounded UTF-8
plain cue documents, timing and nonempty text; overlapping cues are allowed.
SRT indices must start at one and be consecutive. VTT identifiers are accepted;
settings, styles, regions, notes and metadata headers are not supported. Input
bytes remain unchanged. Validation is not HTML sanitization or rendering.

`validate_hls_webvtt_segment` accepts bounded plain WebVTT segments with optional
`X-TIMESTAMP-MAP`. `merge_hls_webvtt` normalizes cue timestamps relative to the
primary MPEG-TS presentation clock (33-bit wrapping arithmetic), removes identical
boundary cues and validates the resulting standalone document. Missing maps use
zero LOCAL/MPEGTS as specified, not an inferred per-segment shift. Aggregate input
and output are capped at 1 MiB/4096 cues. Styles, regions and cue settings remain
unsupported. `mpegts_start_pts` reads only a bounded TS prefix; callers own fetching.

## Local remux (explicit opt-in)

`remux_local(explicit_absolute_ffmpeg_path, &MergePlan, &CancellationToken)`
stages engine-owned, immutable verified local regular files using bounded buffers
and a caller-provided aggregate input-byte budget. Ordered MPEG-TS segments or
initialization-first fragmented MP4 objects form each track. Separate tracks and
validated SRT/VTT subtitles are explicitly mapped into MP4 or Matroska; subtitles
are converted to mov_text or SRT, with optional language labels. Audio/video are
stream-copied, not transcoded; incompatible codec/container combinations fail.
Track selection can map all streams (legacy), video only or audio only, with a
bounded explicit relative offset. This avoids leaking muxed audio when a separate
HLS audio rendition was selected.

No PATH discovery, shell, source URL, concat script or external codec download is
used. ffmpeg is an explicitly trusted user-configured executable, not bundled.
Demuxers and protocols are restricted, MOV external data references are disabled,
direct UNC/device paths and symlink leaves are rejected, and stderr is never
logged. Callers must protect input paths/directories from concurrent mutation;
this API is not a filesystem sandbox. Cancellation covers staging waits and
kills/reaps the child. Private staging is removed after completion. Output is an
unverified temporary `MergeArtifact` removed on drop, **never a destination**.
It may reside on another volume; the engine must stage at the destination and
verify the exact staged bytes before its same-directory publication protocol.
There is no output hash, publication, durable job state, or crash recovery here.
Input budget bounds staging; subprocess CPU/output growth still needs caller/OS
resource policy for hostile media. Use a maintained ffmpeg build.

## Engine boundary and remaining gates

All networking belongs to the engine, including bounded cancellable fetching,
redirect-origin authorization and credential handling. A parsed URL is **not**
network authorization: cross-origin/CDN references require explicit policy.
Durable project/segment identity, checkpointing, resume, exact merged-output hash
verification and verified no-clobber publication remain engine-owned. Do not
concatenate arbitrary segments and call the result verified media.

Run `cargo test -p osdm-media-capture` and
`cargo clippy -p osdm-media-capture --all-targets -- -D warnings`.
Real generated local TS/fMP4, separate-track, MP4/Matroska subtitle round-trip
tests are opt-in: set absolute `OSDM_TEST_FFMPEG` and `OSDM_TEST_FFPROBE`, then run
`cargo test -p osdm-media-capture --test remux -- --ignored`.
These tests do not establish the engine's exact-output-hash/publication exit gate
or declare P3 complete.
