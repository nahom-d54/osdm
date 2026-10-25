import { extensionOf } from "@/lib/format";
import { cn } from "cn";

const SIZES = {
  sm: "size-7 rounded-[6px] text-[8px]",
  md: "size-9 rounded-md text-[9px]",
  lg: "size-11 rounded-lg text-[10px]",
} as const;

/** A file's extension, set like a label on a physical folder. */
export function FileBadge({ name, size = "md" }: { name: string; size?: keyof typeof SIZES }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex shrink-0 items-center justify-center border border-line-soft bg-raise font-mono font-medium tracking-[0.04em] text-muted-foreground",
        SIZES[size],
      )}
    >
      {extensionOf(name)}
    </span>
  );
}
