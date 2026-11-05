import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

interface ToggleProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  description?: string;
  disabled?: boolean;
  badge?: ReactNode;
}

export function Toggle({ label, checked, onChange, description, disabled = false, badge }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cn("toggle-row", checked && "is-on", disabled && "is-disabled")}
      onClick={() => onChange(!checked)}
    >
      <span className="toggle-copy">
        <span className="toggle-label">
          {label}
          {badge}
        </span>
        {description ? <span className="toggle-description">{description}</span> : null}
      </span>
      <span className="switch" aria-hidden="true">
        <span className="switch-knob" />
      </span>
    </button>
  );
}
