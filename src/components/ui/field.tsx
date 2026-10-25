import type { ReactNode } from "react";
import { cn } from "cn";

import { Label } from "@/components/ui/label";

export function Field({
  label,
  htmlFor,
  hint,
  trailing,
  children,
}: {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  trailing?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={htmlFor} className="text-[10px] font-medium uppercase tracking-[0.1em] text-muted-foreground">
          {label}
        </Label>
        {trailing}
      </div>
      {children}
      {hint ? <p className="text-[10.5px] leading-relaxed text-faint">{hint}</p> : null}
    </div>
  );
}

export function ErrorNote({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <p
      role="alert"
      className={cn(
        "rounded-md border border-danger/25 bg-danger-soft px-3 py-2 text-[11.5px] leading-relaxed text-danger",
        className,
      )}
    >
      {children}
    </p>
  );
}
