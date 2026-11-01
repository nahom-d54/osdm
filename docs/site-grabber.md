# Site Grabber — scope and safety policy

This is the P3 scope decision for website retrieval. Implementation and validation
status are recorded separately in [P3 retrieval](p3-retrieval.md); this policy is
not a claim that every Site Grabber workflow has passed its release gate.

## Explicit projects, not general-purpose scraping

- A project starts with an explicitly confirmed HTTP(S) seed and path scope on
  exactly one origin. A discovered link, redirect or HTML base element cannot
  broaden that authority. Credentials in URLs are rejected.
- No headless browser, form submission, JavaScript evaluation, login automation,
  browser-cookie export or cross-origin credential forwarding is permitted.
- Page and file filters narrow the approved scope. Changing filters must not
  implicitly broaden origin/path authority or discard already published files.
- Discovery is bounded and reviewable. Resource selection is an explicit action;
  finding a media element or link does not authorize downloading it.

## Robots and request boundaries

- Fetch the origin's robots policy before exploring pages. Respect applicable
  `OSDM` and wildcard restrictions; errors fail closed rather than granting scope.
- Requests belong to the headless engine and share its global and host
  connection limits, bandwidth budget, cooldowns, cancellation and shutdown.
- Disable automatic redirects. Validate and account for each hop before sending
  another request; refuse scope escapes and HTTPS downgrades. Canonical visited
  URLs and finite budgets terminate redirect/query/link cycles.
- Preserve query-string identity because it can select different content. Strip
  fragments for fetch deduplication. Never log source URLs or use signed queries
  as analytics keys.

## Storage and continuation

- Persist the frontier and in-flight intent before requesting work. Restore only
  validated project state, paused until the user resumes it.
- Finite page, file, depth and byte limits are required. Account for response
  bodies and reserve selected resources against the same byte ceiling; unknown
  sizes do not mean zero bytes. Enforce limits while streaming, not merely from
  an untrusted `Content-Length`.
- The byte limit is a **payload/storage budget**, not a metered-network traffic
  quota. It conservatively accounts for discovery bodies and reserves the exact
  maximum stored payload of selected files. Transfer retries/retransmissions are
  not added a second time to selected-file reservations; ordinary checkpointed
  resume remains available. Every selected transfer must retain its durable
  length cap and scope/redirect restrictions after restart. No claim of a WAN
  byte quota is made.
- Relative output names must be safe on Windows and resist traversal, reserved
  names and collisions. Remote paths never become arbitrary filesystem paths.
- Selected downloads must use the ordinary engine's durable checkpoints,
  validator revalidation, integrity computation and no-overwrite publication.
  Deleting a project must not implicitly delete downloaded files.
- Pause/cancel/shutdown must drain project workers. Restart must not silently
  resume exploration, broaden filters or repeat a consumed schedule decision.

## Acceptance evidence

P3 requires a demonstrated pause/restart/resume mirror within budgets and
filters, with redirect loops and repeated links terminating. Tests must also
cover denied robots policy, origin/authentication boundaries, unknown or changing
resource lengths, malformed input and destination conflicts. A link parser or a
green UI-only test is not sufficient evidence of a completed mirror.
