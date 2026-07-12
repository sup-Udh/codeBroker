import React from 'react';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

// Fixed categorical order — reused identically across every chart in the
// dashboard so a given series/tool/language always maps to the same color
// no matter which page it appears on.
export const CHART_COLORS = ['#ff6b35', '#3b82f6', '#22c55e', '#a855f7', '#eab308', '#ec4899'];

export function Card({ className, children }: { className?: string; children: React.ReactNode }) {
  return (
    <div
      className={cn(
        'relative bg-[#111113] border border-[#1f1f22] p-6 rounded-2xl',
        'shadow-[0_1px_0_0_rgba(255,255,255,0.03)_inset]',
        className
      )}
    >
      {children}
    </div>
  );
}

export function SectionHeading({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle?: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex justify-between items-start gap-4">
      <div>
        <h2 className="text-xl font-semibold tracking-tight text-[#fafafa]">{title}</h2>
        {subtitle && <p className="text-sm text-[#71717a] mt-1">{subtitle}</p>}
      </div>
      {action}
    </div>
  );
}

export function StatCard({
  label,
  value,
  icon,
  sublabel,
  accent = false,
  className,
}: {
  label: string;
  value: string | number;
  icon: React.ReactNode;
  sublabel?: string;
  accent?: boolean;
  className?: string;
}) {
  return (
    <Card className={cn('flex flex-col gap-4 overflow-hidden group', className)}>
      {accent && (
        <div className="absolute -top-10 -right-10 w-28 h-28 rounded-full bg-[#ff6b35]/10 blur-2xl group-hover:bg-[#ff6b35]/15 transition-colors" />
      )}
      <div className="flex items-center gap-2.5 text-[#71717a] relative z-10">
        <div
          className={cn(
            'p-2 rounded-lg',
            accent ? 'bg-[#ff6b35]/10 text-[#ff6b35]' : 'bg-[#1f1f22] text-[#fafafa]'
          )}
        >
          {React.isValidElement(icon) ? React.cloneElement(icon as React.ReactElement, { className: 'w-4 h-4' }) : icon}
        </div>
        <span className="font-medium text-sm truncate">{label}</span>
      </div>
      <div className="relative z-10">
        <div className="text-3xl font-semibold tracking-tight truncate text-[#fafafa]">{value}</div>
        {sublabel && <div className="text-xs text-[#71717a] mt-1.5">{sublabel}</div>}
      </div>
    </Card>
  );
}

export function Badge({
  children,
  variant = 'default',
}: {
  children: React.ReactNode;
  variant?: 'default' | 'success' | 'warning' | 'error' | 'info';
}) {
  const variants: Record<string, string> = {
    default: 'bg-[#1f1f22] text-[#a1a1aa]',
    success: 'bg-[#22c55e]/10 text-[#22c55e] border border-[#22c55e]/20',
    warning: 'bg-[#eab308]/10 text-[#eab308] border border-[#eab308]/20',
    error: 'bg-[#ef4444]/10 text-[#ef4444] border border-[#ef4444]/20',
    info: 'bg-[#3b82f6]/10 text-[#3b82f6] border border-[#3b82f6]/20',
  };
  return (
    <span className={cn('px-2.5 py-0.5 rounded-full text-xs font-medium whitespace-nowrap', variants[variant])}>
      {children}
    </span>
  );
}

export function StatusDot({ status }: { status: 'success' | 'warning' | 'error' | 'idle' }) {
  const colors: Record<string, string> = {
    success: 'bg-[#22c55e]',
    warning: 'bg-[#eab308]',
    error: 'bg-[#ef4444]',
    idle: 'bg-[#71717a]',
  };
  const pulse = status === 'success';
  return (
    <span className="relative inline-flex w-2 h-2">
      {pulse && <span className={cn('absolute inline-flex h-full w-full rounded-full opacity-60 animate-ping', colors[status])} />}
      <span className={cn('relative inline-flex rounded-full w-2 h-2', colors[status])} />
    </span>
  );
}

export function EmptyState({ icon, title, subtitle }: { icon?: React.ReactNode; title: string; subtitle?: string }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-16 text-[#71717a]">
      {icon && <div className="mb-4 opacity-50">{icon}</div>}
      <p className="font-medium text-[#a1a1aa]">{title}</p>
      {subtitle && <p className="text-sm mt-1 max-w-sm">{subtitle}</p>}
    </div>
  );
}

export function LoadingState({ label = 'Loading…' }: { label?: string }) {
  return (
    <div className="flex items-center justify-center h-full py-24 text-[#71717a] gap-3">
      <span className="w-3 h-3 rounded-full border-2 border-[#1f1f22] border-t-[#ff6b35] animate-spin" />
      {label}
    </div>
  );
}

export function ProgressBar({ value, max = 100, color = '#ff6b35' }: { value: number; max?: number; color?: string }) {
  const pct = Math.min(100, Math.max(0, (value / max) * 100));
  return (
    <div className="w-full bg-[#1f1f22] h-1.5 rounded-full overflow-hidden">
      <div className="h-full rounded-full transition-all duration-500" style={{ backgroundColor: color, width: `${pct}%` }} />
    </div>
  );
}

export const tooltipStyle = {
  contentStyle: {
    backgroundColor: '#18181b',
    borderColor: '#27272a',
    borderRadius: '10px',
    boxShadow: '0 8px 24px rgba(0,0,0,0.5)',
    fontSize: '12px',
    padding: '8px 12px',
  },
  labelStyle: { color: '#a1a1aa', marginBottom: 4, fontWeight: 500 },
  itemStyle: { color: '#fafafa' },
};
