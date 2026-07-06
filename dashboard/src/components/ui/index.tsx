import React from 'react';
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function Card({ className, children }: { className?: string; children: React.ReactNode }) {
  return (
    <div className={cn("bg-[#111113] border border-[#1f1f22] p-6 rounded-xl", className)}>
      {children}
    </div>
  );
}

export function StatCard({ 
  label, 
  value, 
  icon, 
  trend, 
  trendUp = true,
  className 
}: { 
  label: string; 
  value: string | number; 
  icon: React.ReactNode; 
  trend?: string;
  trendUp?: boolean;
  className?: string;
}) {
  return (
    <Card className={cn("flex flex-col gap-4", className)}>
      <div className="flex items-center justify-between text-[#71717a]">
        <div className="flex items-center gap-2">
          <div className="p-2 bg-[#1f1f22] rounded-lg text-[#fafafa]">
            {React.isValidElement(icon) ? React.cloneElement(icon as React.ReactElement, { className: "w-4 h-4" }) : icon}
          </div>
          <span className="font-medium text-sm truncate">{label}</span>
        </div>
      </div>
      <div>
        <div className="text-3xl font-semibold tracking-tight truncate">{value}</div>
        {trend && (
          <div className={cn("text-sm mt-1 flex items-center gap-1", trendUp ? "text-[#22c55e]" : "text-[#ff6b35]")}>
            {trendUp ? '↗' : '↘'} {trend} vs yesterday
          </div>
        )}
      </div>
    </Card>
  );
}

export function Badge({ children, variant = 'default' }: { children: React.ReactNode, variant?: 'default' | 'success' | 'warning' | 'error' }) {
  const variants = {
    default: "bg-[#1f1f22] text-[#fafafa]",
    success: "bg-green-500/10 text-green-500 border border-green-500/20",
    warning: "bg-yellow-500/10 text-yellow-500 border border-yellow-500/20",
    error: "bg-red-500/10 text-red-500 border border-red-500/20",
  };
  return (
    <span className={cn("px-2.5 py-0.5 rounded-full text-xs font-medium", variants[variant])}>
      {children}
    </span>
  );
}
