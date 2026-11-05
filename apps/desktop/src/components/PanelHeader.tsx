import type { ReactNode } from "react";

interface PanelHeaderProps {
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
}

export function PanelHeader({ icon, title, description, action }: PanelHeaderProps) {
  return (
    <header className="panel-head">
      <div className="panel-head__icon">{icon}</div>
      <div className="panel-head__copy">
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
      {action ? <div className="panel-head__action">{action}</div> : null}
    </header>
  );
}
