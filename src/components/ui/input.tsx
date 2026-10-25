import * as React from "react";
import { Input as InputPrimitive } from "@base-ui/react/input";
import { cn } from "cn";

function Input({
  className,
  type,
  invalid,
  ...props
}: React.ComponentProps<"input"> & { invalid?: boolean }) {
  return (
    <InputPrimitive
      type={type}
      data-slot="input"
      aria-invalid={invalid || undefined}
      className={cn(
        "h-9 w-full min-w-0 rounded-md border border-line bg-panel/70 px-3 py-1 text-[13px] text-foreground transition-colors outline-none placeholder:text-faint hover:border-faint/50 focus-visible:border-brand disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-danger/70 aria-invalid:focus-visible:border-danger",
        className,
      )}
      {...props}
    />
  );
}

export { Input };
