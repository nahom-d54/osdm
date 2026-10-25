import { Toggle as TogglePrimitive } from "@base-ui/react/toggle";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "cn";

const toggleVariants = cva(
  "group/toggle inline-flex select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-[5px] text-[11.5px] font-medium text-muted-foreground transition-colors outline-none hover:bg-raise hover:text-foreground disabled:pointer-events-none disabled:opacity-50 data-pressed:bg-panel data-pressed:text-ink-strong data-pressed:shadow-[0_1px_2px_rgba(0,0,0,0.2)] [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default: "bg-transparent",
        outline: "border border-line bg-transparent hover:bg-raise",
      },
      size: {
        sm: "h-6 min-w-6 px-2",
        default: "h-7 min-w-7 px-2.5",
        lg: "h-8 min-w-8 px-3",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Toggle({
  className,
  variant = "default",
  size = "default",
  ...props
}: TogglePrimitive.Props & VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive
      data-slot="toggle"
      className={cn(toggleVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Toggle, toggleVariants };
