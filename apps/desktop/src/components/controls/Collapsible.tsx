import { useId, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "../../lib/cn";

interface CollapsibleProps {
  title: string;
  subtitle?: string;
  defaultOpen?: boolean;
  trailing?: ReactNode;
  children: ReactNode;
}

export function Collapsible({ title, subtitle, defaultOpen = false, trailing, children }: CollapsibleProps) {
  const [open, setOpen] = useState(defaultOpen);
  const contentId = useId();

  return (
    <div className={cn("collapsible", open && "is-open")}>
      <div className="collapsible__head">
        <button
          type="button"
          className="collapsible__toggle"
          aria-expanded={open}
          aria-controls={contentId}
          onClick={() => setOpen((current) => !current)}
        >
          <span className="collapsible__titles">
            <span className="collapsible__title">{title}</span>
            {subtitle ? <span className="collapsible__subtitle">{subtitle}</span> : null}
          </span>
          <ChevronDown size={14} className="collapsible__chevron" aria-hidden="true" />
        </button>
        {trailing ? <span className="collapsible__trailing">{trailing}</span> : null}
      </div>
      <div id={contentId} className="collapsible__body">
        <div className="collapsible__inner">{children}</div>
      </div>
    </div>
  );
}
