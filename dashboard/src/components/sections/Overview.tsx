import React, { useEffect, useState, useCallback } from 'react';
import { Layers, Zap, Clock, Code2, Cpu, Database, Activity, FileCode, Network, Link, Box } from 'lucide-react';
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { api, RepositoryOverview, SystemHealth } from '../../providers/api';
import { Card, StatCard, Badge } from '../ui';
import { useLiveEvents } from '../../hooks/useLiveEvents';

export function Overview() {
  const [data, setData] = useState<RepositoryOverview | null>(null);
  const [health, setHealth] = useState<SystemHealth | null>(null);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(() => {
    Promise.all([api.getOverview(), api.getSystemHealth()]).then(([overview, systemHealth]) => {
      setData(overview);
      setHealth(systemHealth);
      setLoading(false);
    });
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  useLiveEvents((type) => {
    if (type === 'mcp_activity' || type === 'index_update') {
      loadData();
    }
  });

  if (loading || !data || !health) {
    return <div className="text-[#71717a] flex items-center justify-center h-full">Loading insights...</div>;
  }

  const formatNumber = (num: number) => {
    if (num >= 1000000) return (num / 1000000).toFixed(2) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'k';
    return num.toString();
  };

  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.round(ms / 60000)}m`;
  };

  const formatBytes = (bytes: number) => {
    if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(2) + ' GB';
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(2) + ' MB';
    if (bytes >= 1024) return (bytes / 1024).toFixed(2) + ' KB';
    return bytes + ' B';
  };

  return (
    <div className="space-y-8 animate-in fade-in duration-500">
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <StatCard label="Tokens Used" value={formatNumber(data.tokensUsed)} icon={<Cpu />} />
        <StatCard label="Est. Cost Saved" value={`$${(data.estCostSavedCents / 100).toFixed(2)}`} icon={<CoinsIcon />} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <Card className="lg:col-span-2">
          <h3 className="text-lg font-medium mb-6">Token Usage Over Time</h3>
          <div className="h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={data.tokenUsageGraph}>
                <defs>
                  <linearGradient id="colorUsed" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor="#71717a" stopOpacity={0.3}/>
                    <stop offset="95%" stopColor="#71717a" stopOpacity={0}/>
                  </linearGradient>
                  <linearGradient id="colorSaved" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor="#ff6b35" stopOpacity={0.2}/>
                    <stop offset="95%" stopColor="#ff6b35" stopOpacity={0}/>
                  </linearGradient>
                </defs>
                <XAxis dataKey="time" stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} />
                <YAxis stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} tickFormatter={(v) => `${v/1000}k`} />
                <Tooltip 
                  contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                  itemStyle={{ color: '#fafafa' }}
                />
                <Area type="monotone" dataKey="used" stroke="#71717a" fillOpacity={1} fill="url(#colorUsed)" name="Tokens Used" />
                <Area type="monotone" dataKey="saved" stroke="#ff6b35" fillOpacity={1} fill="url(#colorSaved)" name="Tokens Saved" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </Card>

        <Card className="flex flex-col">
          <h3 className="text-lg font-medium mb-6">System Health</h3>
          <div className="flex-1 space-y-6">
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2"><Database className="w-4 h-4" /> Database</span>
              <Badge variant={health.databaseStatus === 'Healthy' ? 'success' : 'error'}>{health.databaseStatus}</Badge>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2"><Zap className="w-4 h-4" /> Cache Hit Rate</span>
              <span className="text-[#fafafa] font-medium">{health.cacheHitRate.toFixed(1)}%</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2"><Clock className="w-4 h-4" /> Index Freshness</span>
              <span className="text-[#fafafa] font-medium">{formatDuration(health.indexFreshnessMs)} ago</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2"><Activity className="w-4 h-4" /> MCP Server</span>
              <Badge variant={health.mcpServerStatus === 'Connected' ? 'success' : 'error'}>{health.mcpServerStatus}</Badge>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2"><Layers className="w-4 h-4" /> DB Size</span>
              <span className="text-[#fafafa] font-medium">{health.sqliteSizeMb.toFixed(1)} MB</span>
            </div>
          </div>
        </Card>
      </div>

      <div>
        <h3 className="text-xl font-semibold mb-4">Repository Overview</h3>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><FileCode className="w-3 h-3" /> Files</div>
            <div className="text-2xl font-semibold">{formatNumber(data.filesIndexed)}</div>
          </div>
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Box className="w-3 h-3" /> Symbols</div>
            <div className="text-2xl font-semibold">{formatNumber(data.symbols)}</div>
          </div>
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Link className="w-3 h-3" /> Edges</div>
            <div className="text-2xl font-semibold">{formatNumber(data.relationships)}</div>
          </div>
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Network className="w-3 h-3" /> Communities</div>
            <div className="text-2xl font-semibold">{formatNumber(data.communities)}</div>
          </div>
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Code2 className="w-3 h-3" /> Languages</div>
            <div className="text-2xl font-semibold">{formatNumber(data.languages)}</div>
          </div>
          <div className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22]">
            <div className="text-[#71717a] text-xs mb-1 uppercase tracking-wider font-semibold flex items-center gap-1"><Database className="w-3 h-3" /> Embeddings</div>
            <div className="text-2xl font-semibold">{formatNumber(data.embeddings)}</div>
          </div>
        </div>
      </div>
    </div>
  );
}

function CoinsIcon() {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="lucide lucide-coins">
      <circle cx="8" cy="8" r="6" />
      <path d="M18.09 10.37A6 6 0 1 1 10.34 18" />
      <path d="M7 6h1v4" />
      <path d="m16.71 13.88.7.71-2.82 2.82" />
    </svg>
  );
}
