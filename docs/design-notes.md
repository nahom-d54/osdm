# P1 UI — design notes

## Running

```text
pnpm install
pnpm dev        # browser tab, mock engine (no Rust needed)
pnpm typecheck  # tsc --noEmit
pnpm build      # typecheck + vite build
```

Inside the future Tauri webview the same code binds `TauriBridge`
automatically (see `docs/bridge-contract.md`).

---

Direction: **instrument bench**. OSDM's product promise is measured, durable,
honest transfer, so the shell reads like a calibrated instrument: cool
drafting-slate surfaces, monospaced numbers everywhere measurements live, and
one signature element carrying the durability story. Everything else stays
quiet.

## Tokens (src/styles.css)

- Dark theme ("instrument", default): bg `#0E141B`, panel `#141B24`, raise
  `#1B2430`, line `#263140`, ink `#E7EEF6`, muted `#8FA2B6`, accent
  `#55A0E8` (signal blue), ok `#57C48F`, warn `#D9A23E`, danger `#DE6A5C`.
- Light theme ("millboard"): same hue family on cool paper: bg `#EDF0F4`,
  panel `#FFFFFF`, accent `#1E66C7`.
- Both themes map through `@theme` names (`bg-panel`, `text-muted`,
  `border-line`, …) so components never hardcode hex.

Deliberately **not**: cream + terracotta serif, near-black + acid green, or
broadsheet hairlines — the three templated looks. Rounded 6/10 px, one hue
family, status colors reserved for semantics.

## Type

- Display: **Familjen Grotesk** (wordmark, dialog titles, panel headers).
- Body: **IBM Plex Sans**.
- Data: **IBM Plex Mono** — every byte count, speed, ETA, percent, range,
  URL, and path. Numbers are telemetry in this product, and the face choice
  is the second-most distinctive decision after the ribbon.
- Self-hosted via `@fontsource` packages (bundled, offline — fits
  local-first privacy; no font CDN).

## Layout (PLAN §5.2, followed exactly)

```
┌─────────┬────────────────────────────────────────────┐
│ Sidebar │ Toolbar: add · resume-all/pause-all · stop │
│ Library ├────────────────────────┬───────────────────┤
│ Active  │ Download list          │ Details drawer    │
│ Queued  │ (rows w/ ribbon)       │ (segments, err)   │
│ Finished├────────────────────────┴───────────────────┤
│         │ Status: throughput · counts · speed limit  │
└─────────┴────────────────────────────────────────────┘
```

## Signature: the segment ribbon (`src/components/SegmentRibbon.tsx`)

Per row, the engine's real `segments[]` render as one thin bar:

- **committed** bytes → solid green (durable; survives a crash),
- **live** bytes → translucent blue pulse (in flight; reset to committed on
  recovery), separated by 1-px keylines at segment boundaries,
- unknown-length single streams → flowing stripes while active.

Until `src-tauri/` exists, the `MockBridge`'s simulated segment layout drives
the same component, so the semantics match the engine's contract (--not just
a fake percent bar).

## Motion

Three keyframes only, all content-serving: `live-pulse` (in-flight bytes),
`flow-stripes` (unknown-length stream), `rise-in` (dialog mount). All
disabled under `prefers-reduced-motion: reduce`.

## Accessibility status (P1 §8.3 gate)

Implemented: `:focus-visible` global outline; all state chips carry a text
label (never color alone); `aria-label`s on every icon button; ribbon uses
`role="progressbar"` with committed-byte values; dialogs are `aria-modal` and
labelled; list rows are focusable with roving tabindex, ↑/↓ move, Enter opens
details, Space pauses/resumes; shortcuts `N` (add), `/` or `Ctrl+F`
(search), `D` (details panel); status bar announces throughput with
`aria-live="polite"`.

Chromium regression coverage now checks keyboard dialog focus return, scaled
layouts, reduced motion, and axe WCAG AA contrast/labels for populated lists and
settings in both themes. Still owed before the release gate: native keyboard-only
walkthrough, Narrator pass, and Windows scaling verification (§8.3). Localization
is skipped at the user's request.

## Writing

Sentence case, plain verbs, one job per element: "Add download", "Resume",
"Stop", "Show in folder", "Delete entry and partial file". Errors state what
happened plus the next action ("safe progress is kept. Retry to continue.").
Empty states explain what will appear there, not how the user should feel
about it.

## Notes for future passes

- Native settings are authoritative SQLite-backed JSON; Zustand/localStorage
  remains the browser mock's persistence adapter. See `docs/bridge-contract.md`.
- Hand-rolled primitives in `src/components/ui/` follow shadcn-ish APIs;
  swapping in the actual shadcn registry later should be mechanical. Kept
  dependency-light per PLAN §1.2.
- Category navigation and durable assignments, descriptions/destination editing,
  Download Later, multiselect, column/toolbar controls, desktop dialogs, and
  opt-in completion actions are implemented. The transfer desk's existing type,
  spacing, surfaces, and durable segment visualization remain the visual vocabulary.
- Browser capture remains P2. Multiple languages are excluded from this pass.
