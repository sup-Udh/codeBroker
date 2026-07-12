import React from 'react';
import { 
  LayoutDashboard, 
  Coins, 
  Activity, 
  FolderTree, 
  Network, 
  Wrench, 
  AlertCircle, 
  Settings,
  TerminalSquare
} from 'lucide-react';
import { Section } from '../../App';
import { api, RepositoryOverview } from '../../providers/api';
import { useState, useEffect, useCallback } from 'react';

interface SidebarProps {
  activeSection: Section;
  onSelect: (section: Section) => void;
}

export function Sidebar({ activeSection, onSelect }: SidebarProps) {
  const [data, setData] = useState<RepositoryOverview | null>(null);

  const loadData = useCallback(() => {
    api.getOverview().then(setData).catch(console.error);
  }, []);

  useEffect(() => {
    loadData();
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

  return (
    <aside className="w-64 border-r border-[#1f1f22] bg-[#09090b] flex flex-col flex-shrink-0">
      <div className="h-16 flex items-center px-6 border-b border-[#1f1f22]">
        <div className="flex items-center gap-3 text-[#ff6b35]">
          <TerminalSquare className="w-6 h-6" />
          <span className="font-semibold text-lg tracking-tight text-[#fafafa]">CodeBroker</span>
        </div>
      </div>
      
      <nav className="flex-1 overflow-y-auto py-6 px-4 space-y-1">
        {navItems.map((item) => (
          <button
            key={item.id}
            onClick={() => onSelect(item.id)}
            className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-colors duration-200 ${
              activeSection === item.id
                ? 'bg-[#111113] border border-[#ff6b35]/30 text-[#ff6b35] shadow-[0_0_15px_rgba(255,107,53,0.1)]'
                : 'text-[#71717a] hover:bg-[#111113] hover:text-[#fafafa] border border-transparent'
            }`}
          >
            {item.icon}
            <span className="font-medium">{item.label}</span>
          </button>
        ))}
      </nav>

      <div className="p-4 border-t border-[#1f1f22]">
        <div className="px-3 py-3 bg-[#111113] rounded-lg border border-[#1f1f22]">
          <div className="text-xs text-[#71717a] mb-1 uppercase tracking-wider font-semibold">Workspace</div>
          <div className="text-sm font-medium truncate" title={data?.workspacePath || "Loading..."}>
            {data?.workspaceName || "Loading..."}
          </div>
          <div className="text-xs text-[#71717a] mt-2 flex justify-between">
            <span>v1.2.0</span>
            <span className="text-[#22c55e]">Active</span>
          </div>
        </div>
      </div>
    </aside>
  );
}
