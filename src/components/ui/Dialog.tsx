import { useEffect, type ReactNode } from "react";

interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  width?: string;
}

/** Modal dialog: labelled by its title, Escape closes, focus lands on the panel. */
export function Dialog({ open, title, onClose, children, width = "26rem" }: DialogProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-[12vh]"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{ width }}
        className="rise-in rounded-[var(--radius-panel)] border border-line bg-panel shadow-[0_16px_48px_rgba(0,0,0,0.35)] focus:outline-none"
        ref={(node) => node?.focus()}
      >
        <header className="flex items-center justify-between border-b border-line-soft px-5 pt-4 pb-3">
          <h2 className="font-display text-[15px] font-semibold text-ink-strong">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded p-1 text-faint hover:bg-raise hover:text-ink"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" aria-hidden="true">
              <path d="m2 2 8 8M10 2 2 10" />
            </svg>
          </button>
        </header>
        {children}
      </div>
    </div>
  );
}
