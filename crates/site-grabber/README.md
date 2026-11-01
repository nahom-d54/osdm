# Site grabber core (P3 foundation, not complete P3)

Network-free deterministic exploration and review state for an engine-owned site
grabber. No HTTP client, disk writes, JavaScript execution, forms, login, cookie
management, automatic redirects, or resource downloads are implemented here.

## Integration API

1. Build wizard `ProjectConfig` (`source`, `scope.path_prefix`, `filters`, `budgets`)
   and `CreateProjectRequest { config, confirmed }`; call `Project::create`.
   `snapshot()` returns UI fields/resources, not a resumable persistence format.
   The lower-level `Config`/`new` API is also supported. Use an explicit HTTP(S)
   seed and absolute path prefix ending in `/`, extension allowlist and excludes.
2. Show the project scope/budgets and obtain explicit user consent before `confirm`.
3. `next_fetch` returns a stable `FetchIntent`; repeating it, including after
   pause/serialize/restore/resume, repeats the in-flight intent. Perform exactly
   that request through engine-owned budgeted transport, with redirects disabled,
   no credentials/cookies/authorization, and streaming body cap `max_bytes`.
   Requests must identify as `OSDM`. Bound headers and network waiting in the engine.
4. `accept(FetchedResponse)` charges every body byte (including robots, failures,
   and redirects). `received_bytes` must equal the buffered body length; truncation,
   mismatch, over-budget response or automatic redirect stops fail-closed. On a
   transport failure the caller can retry the same intent through engine retry
   budgets, pause, or submit a synthetic failure status with the actual body count.
5. Inspect `resources()`. Discovered files have `known_bytes: None`, never an
   assumed zero. `request_metadata(url)` schedules one persisted
   `FetchKind::ResourceMetadata` **HEAD** intent, prioritized ahead of queued pages
   (an existing in-flight intent is never replaced). Fetch it through the same
   engine/cap/accounting path, with identity encoding and no Range. Return parsed
   `content_length` through `accept`. Only a 200 exact-URL HEAD with no response
   body establishes known length. Redirect/error/unknown length stays unselectable;
   do not auto-follow or fall back to GET. All received HEAD bytes are still charged.
6. Nothing is selected automatically. On explicit user review call
   `select(url, Some(size))`; size must match successful stored HEAD metadata.
   Unknown lengths are rejected. Admission reserves the full size and any
   outstanding fetch allowance.
   Reservation is idempotent and cannot be released in this initial core. Only
   the engine may perform selected downloads and must enforce reserved sizes,
   validate redirects with `validate_target` before fetch (do not follow automatically),
   and use conflict-detecting publication. Revalidation failures require new review.

`serde_json::to_string(&project)` persists state; **only** `Project::restore` loads
and validates it. There is deliberately no `Deserialize` implementation on
`Project`. Snapshots and public URL fields are secrets, including signed queries:
protect persisted storage and never log them. `FetchIntent` Debug redacts the URL;
other URL-bearing types do not implement Debug. Restore is structural validation,
not authentication; trusted storage must prevent malicious snapshot tampering.
The core does not promise crash-safe persistence; caller must commit intent before
fetching and resulting state before continuing. Resuming a GET may replay it.

## Scope and policy

- Exact scheme/host/effective port origin and explicit seed path prefix. Excluded
  paths are conservative raw prefixes. Userinfo and non-HTTP(S) URLs are rejected.
  Percent-encoded and repeated-slash paths are conservatively rejected to avoid server decoding and
  traversal ambiguities; query strings are retained byte-for-byte by URL parsing,
  never sorted or stripped. Fragments are removed. URL normalization handles dot
  segments/default ports; different query identities remain different resources.
- Robots is fetched first at the origin's `/robots.txt` (outside the seed prefix).
  Only 200, 404, 410 permit progress; redirects/errors deny exploration. Union all
  matching `OSDM` and `*` disallows, ignoring Allow exceptions conservatively.
  Wildcard `*` works; trailing `$` is conservatively ignored (may deny extra).
  Malformed non-path or percent-encoded Disallow denies all; invalid UTF-8 robots
   stops exploration. A single leading UTF-8 BOM is recognized; unexpected further
   BOMs deny all rather than hiding an agent/directive. Empty Disallow permits. Crawl-delay and
  sitemaps are not implemented; engine must supply conservative throttling.
- HTML `a/area/link[href]` and `img/source/video/audio[src]` only; no base tag,
  scripts, CSS, srcset, forms, or sitemap traversal. Tolerant `scraper` HTML parsing.
  Only extensionless and `.html`/`.htm` pages explored; explicitly allowlisted file
   extensions become review resources. Allowlisting HTML does not disable crawling:
   `.html`/`.htm` URLs can be both crawl pages and reviewable resources, independently
   subject to page/file limits. Reviewing HTML still requires explicit HEAD/selection.
- Every canonical URL is seen once; page identities are tracked separately from
   resource identities, with overlap permitted for HTML. Page slots count distinct
   page URLs, not resource URLs. One pending intent per (URL, request kind) permits a
   page GET and resource HEAD to coexist without duplication. Redirects consume page slots, retain
  depth, and are scheduled only after scope/robots checks; loops terminate.
  Files and pages share the same depth limit. Out-of-scope redirects are discarded
  before fetch. No retries/backoff in this crate.
- Flat deterministic `resource-00001.ext` filenames use ASCII allowlisted extensions,
  not remote path names. Unique within one project; never traversal or Windows
  reserved names. Across projects/existing files the engine **must not overwrite**.

## Exact limits

Defaults: 100 page fetch URLs (including redirect targets, excluding robots), 100
discovered files, 10 MiB combined fetched bodies and selected-file reservations,
depth 4 (seed 0). Configurable ceilings: 10,000 pages, 10,000 files, 10 GiB, depth
16. Zero page/file/byte budgets rejected. One in-flight fetch, FIFO page frontier
with explicit metadata probes prioritized. At most one HEAD per discovered file.
Body caps: 512 KiB HTML/redirect/error/HEAD; 64 KiB robots. At most 2,048 candidate link
elements per page, 8,192 bytes per URL, 128 ASCII extensions (1–16 letters/digits)
and 128 excludes. Restore accepts snapshots at most 64 MiB. Admission additionally
limits the aggregate canonical URL pool (`seen`) to 1 MiB, cached incrementally
in memory and recomputed/validated on restore. If a new admissible link would
exceed that pool, `accept` returns `Error::Budget` and stops with the prior admitted
state preserved and restorable (already accepted links remain). It is not a silent
link drop or an un-restorable successful state. Metadata requests add only a
bounded extra copy of an already admitted URL and need no new URL reservation.

The serialization bound includes up to six copies of each URL (seen, page identity,
resource map key/value, GET and HEAD intents), at worst six JSON bytes per UTF-8
byte: at most 36 MiB. Config is below 7 MiB, aggregate robots text and rule overhead
below 1 MiB, and bounded fixed records/intents/filenames below 8 MiB. Thus every
admitted state stays below 52 MiB, retaining headroom under the unchanged 64 MiB
restore cap. The URL cache is not serialized or trusted from saved input.
Private saved state adds `page_urls`; unambiguous legacy v1 disjoint page/resource
states remain reloadable. Restore checks page intent membership and crawlable
identity separately from metadata intent resource membership. Public APIs and
snapshot DTO fields are unchanged; persistence is validation, not authentication.

HTML is fully parsed
within its body cap. Resource sizes must be known, including zero-byte files.

Core regressions cover serialized pause/resume and in-flight replay, robots,
same-origin/path/auth boundaries, redirects/loops, malformed HTML, query identity,
filename safety, capped discovery, budgets and unknown sizes. Desktop workflow,
engine fetch adapter/persistence/HEAD execution, resource download accounting and
end-to-end P3 acceptance remain integration work.
