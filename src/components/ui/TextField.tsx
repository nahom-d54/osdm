import type { InputHTMLAttributes, ReactNode } from "react";
import { forwardRef } from "react";

export const TextField = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }>(
  ({ invalid, className = "", ...props }, ref) => (
    <input
      ref={ref}
      spellCheck={false}
      autoComplete="off"
      className={`h-9 w-full rounded-[var(--radius-control)] border bg-raise px-2.5 text-[13px] text-ink placeholder:text-faint focus:outline-none focus-visible:border-accent ${
        invalid ? "border-danger/70" : "border-line focus:border-accent"
      } ${className}`}
      {...props}
    />
  ),
);

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-[11px] font-semibold tracking-[0.04em] text-muted uppercase">{label}</span>
      {children}
      {hint != null && <span className="mt-1 block text-[11.5px] text-muted">{hint}</span>}
    </label>
  );
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="rounded-[var(--radius-control)] bg-danger-soft px-2.5 py-2 text-[12px] text-danger">
      {children}
    </p>
  );
}
