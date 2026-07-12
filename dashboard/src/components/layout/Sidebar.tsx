import React, { useState, useEffect, useCallback } from 'react';
import {
  LayoutDashboard,
  Coins,
  Activity,
  FolderTree,
  Network,
  Wrench,
  AlertCircle,
  TerminalSquare,
} from 'lucide-react';
import { Section } from '../../App';
import { api, RepositoryOverview, SystemHealth } from '../../providers/api';
import { StatusDot } from '../ui';

interface SidebarProps {
  activeSection: Section;
  onSelect: (section: Section) => void;
  health: SystemHealth | null;
}

export function Sidebar({ activeSection, onSelect, health }: SidebarProps) {
  const [data, setData] = useState<RepositoryOverview | null>(null);

  const loadData = useCallback(() => {
    api.getOverview().then(setData).catch(console.error);
  }, []);

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 20000);
    return () => clearInterval(interval);
  }, [loadData]);

  const navItems: { id: Section; label: string; icon: React.ReactNode }[] = [
    { id: 'overview', label: 'Overview', icon: <LayoutDashboard className="w-4 h-4" /> },
    { id: 'tokens', label: 'Tokens', icon: <Coins className="w-4 h-4" /> },
    { id: 'activity', label: 'MCP Activity', icon: <Activity className="w-4 h-4" /> },
    { id: 'codebase', label: 'Codebase', icon: <FolderTree className="w-4 h-4" /> },
    { id: 'graph', label: 'Graph', icon: <Network className="w-4 h-4" /> },
    { id: 'tools', label: 'Tools', icon: <Wrench className="w-4 h-4" /> },
    { id: 'errors', label: 'Errors', icon: <AlertCircle className="w-4 h-4" /> },
  ];

  const statusText = !health ? 'Connecting' : health.status === 'healthy' ? 'Active' : health.status === 'stale' ? 'Stale index' : 'No index';
  const statusColor = !health ? 'text-[#71717a]' : health.status === 'healthy' ? 'text-[#22c55e]' : health.status === 'stale' ? 'text-[#eab308]' : 'text-[#ef4444]';

  return (
    <aside className="w-64 border-r border-[#1f1f22] bg-[#09090b] flex flex-col flex-shrink-0">
      <div className="h-16 flex items-center px-6 border-b border-[#1f1f22]">
        <div className="flex items-center gap-3">
          <div className="p-1.5 rounded-lg bg-[#ff6b35]/10 text-[#ff6b35]">
            <TerminalSquare className="w-5 h-5" />
          </div>
          <span className="font-semibold text-lg tracking-tight text-[#fafafa]">CodeBroker</span>
        </div>
      </div>

      <nav className="flex-1 overflow-y-auto py-6 px-4 space-y-1">
        {navItems.map((item) => (
          <button
            key={item.id}
            onClick={() => onSelect(item.id)}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-all duration-200 ${
              activeSection === item.id
                ? 'bg-[#ff6b35]/10 border border-[#ff6b35]/30 text-[#ff6b35] shadow-[0_0_20px_rgba(255,107,53,0.08)]'
                : 'text-[#71717a] hover:bg-[#111113] hover:text-[#fafafa] border border-transparent'
            }`}
          >
            {item.icon}
            <span className="font-medium">{item.label}</span>
          </button>
        ))}
      </nav>

      <div className="p-4 border-t border-[#1f1f22]">
        <div className="px-3 py-3 bg-[#111113] rounded-xl border border-[#1f1f22]">
          <div className="text-xs text-[#71717a] mb-1 uppercase tracking-wider font-semibold">Workspace</div>
          <div className="text-sm font-medium truncate" title={data?.workspacePath || 'Loading...'}>
            {data?.workspaceName || 'Loading...'}
          </div>
          <div className="text-xs text-[#71717a] mt-2 flex justify-between items-center">
            <span className="flex items-center gap-1.5">
              <StatusDot status={!health ? 'idle' : health.status === 'healthy' ? 'success' : health.status === 'stale' ? 'warning' : 'error'} />
              <span className={statusColor}>{statusText}</span>
            </span>
            {health && health.port > 0 && <span>:{health.port}</span>}
          </div>
        </div>
      </div>
    </aside>
  );
}
