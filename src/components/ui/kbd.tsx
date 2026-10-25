import { cn } from "cn";

function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "pointer-events-none inline-flex h-[18px] w-fit min-w-[18px] select-none items-center justify-center rounded-[4px] border border-line bg-raise px-1 font-mono text-[9.5px] font-medium text-faint in-data-[slot=tooltip-content]:border-line in-data-[slot=tooltip-content]:bg-raise in-data-[slot=tooltip-content]:text-muted-foreground",
        className,
      )}
      {...props}
    />
  );
}

function KbdGroup({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="kbd-group" className={cn("inline-flex items-center gap-1", className)} {...props} />;
}

export { Kbd, KbdGroup };
