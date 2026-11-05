import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../../lib/cn";
import { Spinner } from "./Spinner";

type ButtonVariant = "primary" | "secondary" | "ghost" | "subtle";
type ButtonSize = "sm" | "md" | "lg";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: ReactNode;
  loading?: boolean;
  fullWidth?: boolean;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  loading = false,
  fullWidth = false,
  className,
  children,
  disabled,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn("btn", `btn--${variant}`, `btn--${size}`, fullWidth && "btn--full", className)}
      disabled={disabled || loading}
      {...rest}
    >
      {loading ? <Spinner size={size === "lg" ? 16 : 13} /> : icon ? <span className="btn__icon">{icon}</span> : null}
      <span className="btn__label">{children}</span>
    </button>
  );
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  icon: ReactNode;
  active?: boolean;
  tone?: "default" | "danger";
  size?: "sm" | "md";
}

export function IconButton({
  label,
  icon,
  active = false,
  tone = "default",
  size = "md",
  className,
  type = "button",
  ...rest
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      aria-pressed={active || undefined}
      className={cn("icon-btn", `icon-btn--${size}`, active && "is-active", tone === "danger" && "is-danger", className)}
      {...rest}
    >
      {icon}
    </button>
  );
}
