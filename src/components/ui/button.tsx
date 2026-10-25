import { Button as ButtonPrimitive } from "@base-ui/react/button";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "cn";

const buttonVariants = cva(
  "group/button inline-flex shrink-0 select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-md border border-transparent text-[12.5px] font-medium transition-colors duration-150 disabled:pointer-events-none disabled:opacity-45 aria-invalid:border-destructive [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-3.5",
  {
    variants: {
      variant: {
        default:
          "bg-brand text-brand-ink hover:bg-brand-strong",
        secondary:
          "border-line-soft bg-raise text-foreground hover:border-line hover:bg-hover",
        outline:
          "border-line bg-transparent text-foreground hover:bg-raise",
        ghost:
          "text-muted-foreground hover:bg-raise hover:text-foreground",
        destructive:
          "border-danger/25 bg-danger-soft text-danger hover:border-danger/40 hover:bg-danger/15",
        link: "text-brand underline-offset-4 hover:underline",
      },
      size: {
        xs: "h-6 gap-1 rounded-[5px] px-2 text-[11px]",
        sm: "h-7 gap-1.5 rounded-md px-2.5 text-[12px]",
        default: "h-8 px-3",
        lg: "h-9 px-4",
        icon: "size-8",
        "icon-xs": "size-6 rounded-[5px]",
        "icon-sm": "size-7 rounded-md",
        "icon-lg": "size-9 rounded-lg",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant = "default",
  size = "default",
  type = "button",
  ...props
}: ButtonPrimitive.Props & VariantProps<typeof buttonVariants>) {
  return (
    <ButtonPrimitive
      data-slot="button"
      type={type}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
