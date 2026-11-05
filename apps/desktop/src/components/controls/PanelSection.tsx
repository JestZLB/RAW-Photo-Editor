import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

interface PanelSectionProps {
  title: string;
  hint?: string;
  trailing?: ReactNode;
  children: ReactNode;
  className?: string;
}

/** The repeated "title / hint / content" block used by every inspector panel. */
export function PanelSection({ title, hint, trailing, children, className }: PanelSectionProps) {
  return (
    <section className={cn("panel-section", className)}>
      <div className="subsection-head">
        <span className="section-title">{title}</span>
        <span className="subsection-head__right">
          {hint ? <span className="section-hint">{hint}</span> : null}
          {trailing}
        </span>
      </div>
      {children}
    </section>
  );
}
