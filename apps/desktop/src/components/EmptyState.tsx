import type { LucideIcon } from "lucide-react";

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  detail: string;
  action?: React.ReactNode;
}

export function EmptyState({ icon: Icon, title, detail, action }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <span className="empty-state-icon"><Icon size={24} /></span>
      <strong>{title}</strong>
      <p>{detail}</p>
      {action}
    </div>
  );
}
