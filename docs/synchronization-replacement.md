# Synchronization replacement policy

**Decision:** approved by the user. Explicitly enabled synchronization may replace
an unchanged OSDM-owned file at the same destination path. Ordinary downloads still
refuse existing destinations. This is a narrow exception, not a general overwrite
option for enqueue, browser capture, batch imports, or retries.

**Implementation status:** policy approved; replacement is not implemented yet.
Current synchronization continues to publish separate revision files. Changing
this document or `AGENTS.md` must not silently enable replacement for existing
queues or mark the P2 release gate complete.

## Required behavior

- Replacement is an explicit, persisted synchronization choice. Existing queues
  keep revision-preserving behavior unless the user enables replacement.
- Ownership requires a durable record of OSDM's previous publication, matching
  filesystem identity, and matching last-published content. A matching path,
  filename, length, or remote validator alone is not ownership proof.
- An unrelated file, user edit, symlink/reparse-point substitution, changed parent,
  or concurrent destination change is a conflict. Preserve that file and report
  the conflict; never fall back to unconditional replacement.
- Download into a separate working file with normal bounded streaming,
  checkpoints, cancellation, length checks, and integrity verification. Never
  truncate or stream new bytes into the existing destination.
- Persist replacement intent and ownership/recovery metadata before changing
  filesystem names. Keep the old file available until the new file is verified
  and a durable rollback/recovery copy exists.
- Use a tested same-volume atomic replacement protocol with protection against
  races between ownership validation and publication. Unsupported filesystems or
  platforms must refuse replacement, preserving the old file.
- Commit completion and the new published identity/content record before cleaning
  up owned staging or rollback names. Never delete an unrelated file during
  cleanup or recovery. Cleanup failure must not invalidate an already durable
  publication.
- Cancellation before the replacement commit leaves the old destination intact.
  If publication has already committed, report that outcome accurately. Never
  expose a partially written destination or report failed replacement as success.
- Recovery must identify the actual old/new files and finish or roll back an
  interrupted operation without overwriting a file changed by another actor.
  Schema changes require transactional upgrade tests and older-binary rejection.

## Acceptance gate before enabling the feature

Test unchanged-file replacement, user edits, delete/recreate races, concurrent
writers, symlinks/reparse points, parent changes, sharing/permission failures,
disk-full failures, checksum mismatches, and cancellation/crashes around every
intent, staging, publication, completion, and cleanup step. Test graceful and
abrupt restart, failed cleanup retry, schema upgrade/downgrade rejection, and
ordinary-download conflict behavior on supported Windows filesystems.

The original must remain usable on pre-commit failure. After successful commit,
the destination must contain exactly the new verified bytes. Earlier P0/P1/P2
gates must continue passing; policy approval alone is not a gate result.
