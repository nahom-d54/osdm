interface EmptyStateProps {
  title: string;
  body: string;
  hint?: string;
}

/** An empty screen is an invitation to act, not a mood. */
export function EmptyState({ title, body }: EmptyStateProps) {
  return (
    <div className="max-w-88 text-center">
      <div
        aria-hidden="true"
        className="mx-auto mb-4 flex h-10 w-16 items-end justify-center gap-[3px] rounded-md border border-line-soft bg-panel p-2"
      >
        <span className="h-2 w-full rounded-[2px] bg-line" />
        <span className="h-3.5 w-full rounded-[2px] bg-ok" />
        <span className="h-2 w-full rounded-[2px] bg-line" />
      </div>
      <h2 className="font-display text-[15px] font-semibold text-ink-strong">{title}</h2>
      <p className="mx-auto mt-1.5 max-w-72 text-[12.5px] leading-relaxed text-muted">{body}</p>
    </div>
  );
}
