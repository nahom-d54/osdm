import type { ButtonHTMLAttributes, ReactNode } from "react";
import { forwardRef } from "react";

const VARIANTS = {
  primary:
    "bg-accent text-accent-ink font-medium hover:brightness-110 active:brightness-95",
  normal:
    "bg-raise text-ink border border-line hover:border-faint hover:bg-line/40",
  ghost: "text-muted hover:text-ink hover:bg-raise",
  danger: "text-danger hover:bg-danger-soft border border-transparent hover:border-danger/40",
} as const;

const SIZES = {
  sm: "h-7 px-2.5 text-[12px] gap-1.5",
  md: "h-8.5 px-3 text-[13px] gap-2",
} as const;

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof VARIANTS;
  size?: keyof typeof SIZES;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = "normal", size = "md", className = "", type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={`inline-flex items-center rounded-[var(--radius-control)] transition-[background-color,border-color,filter] duration-100 disabled:pointer-events-none disabled:opacity-40 ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
      {...props}
    />
  ),
);

export interface IconButtonProps extends ButtonProps {
  label: string;
  active?: string;
  children: ReactNode;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ label, className = "", variant = "ghost", size = "sm", children, active, ...props }, ref) => (
    <Button
      ref={ref}
      variant={variant}
      size={size}
      aria-label={label}
      title={label}
      aria-pressed={active == null ? undefined : active !== ""}
      className={`w-7 justify-center px-0 ${active ?? ""} ${className}`}
      {...props}
    >
      {children}
    </Button>
  ),
);
