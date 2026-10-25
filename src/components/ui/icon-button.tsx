import type { ReactNode } from "react";
import type { Button as ButtonPrimitive } from "@base-ui/react/button";
import type { VariantProps } from "class-variance-authority";
import { cn } from "cn";

import { Button, type buttonVariants } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

type IconButtonProps = ButtonPrimitive.Props &
  VariantProps<typeof buttonVariants> & {
    label: string;
    shortcut?: string;
    children: ReactNode;
    active?: boolean;
  };

/** An icon-only control that always carries a tooltip and an accessible name. */
export function IconButton({
  label,
  shortcut,
  children,
  className,
  variant = "ghost",
  size = "icon-sm",
  active,
  ...props
}: IconButtonProps) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant={variant}
            size={size}
            aria-label={label}
            aria-pressed={active == null ? undefined : active}
            className={cn(className, active && "bg-brand-soft text-brand hover:bg-brand-soft hover:text-brand")}
            {...(props as ButtonPrimitive.Props)}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>
        {label}
        {shortcut ? <Kbd>{shortcut}</Kbd> : null}
      </TooltipContent>
    </Tooltip>
  );
}
