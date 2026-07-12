import React, { useCallback, useEffect, useState } from 'react';
import { Sidebar } from './components/layout/Sidebar';
import { Overview } from './components/sections/Overview';
import { Tokens } from './components/sections/Tokens';
import { McpActivity } from './components/sections/McpActivity';
import { Codebase } from './components/sections/Codebase';
import { SemanticGraph } from './components/sections/SemanticGraph';
import { Tools } from './components/sections/Tools';
import { Errors } from './components/sections/Errors';
import { api, SystemHealth } from './providers/api';
import { StatusDot } from './components/ui';
import { formatDuration } from './lib/format';

export type Section = 'overview' | 'tokens' | 'activity' | 'codebase' | 'graph' | 'tools' | 'errors';

const SECTION_META: Record<Section, { title: string; subtitle: string }> = {
  overview: { title: 'Overview', subtitle: 'Live index health and AI context savings for this workspace' },
  tokens: { title: 'Tokens', subtitle: 'How much context CodeBroker keeps out of every prompt, and why' },
  activity: { title: 'MCP Activity', subtitle: 'Every tool call an AI agent has made against this index' },
  codebase: { title: 'Codebase', subtitle: 'Structural health, hotspots, and language mix of the indexed repo' },
  graph: { title: 'Graph', subtitle: 'Live dependency graph ranked by real PageRank and connection count' },
  tools: { title: 'Tools', subtitle: 'Usage and efficiency per CodeBroker tool' },
  errors: { title: 'Errors', subtitle: 'Recent failed tool calls, pulled straight from the analytics log' },
};

function App() {
  const [activeSection, setActiveSection] = useState<Section>('overview');
  const [health, setHealth] = useState<SystemHealth | null>(null);

  const loadHealth = useCallback(() => {
    api.getSystemHealth().then(setHealth).catch(() => {});
  }, []);

  useEffect(() => {
    loadHealth();
    const interval = setInterval(loadHealth, 15000);
    return () => clearInterval(interval);
  }, [loadHealth]);

  const renderSection = () => {
    switch (activeSection) {
      case 'overview': return <Overview />;
      case 'tokens': return <Tokens />;
      case 'activity': return <McpActivity />;
      case 'codebase': return <Codebase />;
      case 'graph': return <SemanticGraph />;
      case 'tools': return <Tools />;
      case 'errors': return <Errors />;
      default: return <Overview />;
    }
  };

  const meta = SECTION_META[activeSection];
  const connected = !!health;
  const statusLabel = !health ? 'Connecting…' : health.status === 'healthy' ? 'Index healthy' : health.status === 'stale' ? `${health.staleFiles} file(s) stale` : 'Not indexed';

  return (
    <div className="flex h-screen bg-[#09090b] text-[#fafafa] font-sans overflow-hidden">
      <Sidebar activeSection={activeSection} onSelect={setActiveSection} health={health} />
      <main className="flex-1 flex flex-col min-w-0 overflow-y-auto">
        <header className="h-16 border-b border-[#1f1f22] flex items-center px-8 justify-between flex-shrink-0 bg-[#09090b]/90 backdrop-blur-md sticky top-0 z-10">
          <div className="flex items-center gap-4 min-w-0">
            <h2 className="text-lg font-semibold tracking-tight text-[#fafafa] shrink-0">{meta.title}</h2>
            <span className="text-sm text-[#71717a] truncate hidden md:inline">{meta.subtitle}</span>
          </div>
          <div className="flex items-center gap-3 text-sm shrink-0">
            <div
              className={`flex items-center gap-2 px-3 py-1.5 rounded-full border ${
                connected ? 'border-[#1f1f22] bg-[#111113] text-[#a1a1aa]' : 'border-[#ef4444]/20 bg-[#ef4444]/10 text-[#ef4444]'
              }`}
            >
              <StatusDot status={!health ? 'idle' : health.status === 'healthy' ? 'success' : health.status === 'stale' ? 'warning' : 'error'} />
              <span className="hidden sm:inline">{statusLabel}</span>
            </div>
            {health && (
              <div className="text-[#71717a] hidden lg:flex items-center gap-3">
                <span>:{health.port}</span>
                <span className="text-[#27272a]">·</span>
                <span>up {formatDuration(health.uptimeMs)}</span>
              </div>
            )}
          </div>
        </header>
        <div className="p-8 max-w-[1600px] w-full mx-auto">{renderSection()}</div>
      </main>
    </div>
  );
}

export default App;
