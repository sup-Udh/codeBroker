import React, { useEffect, useState, useCallback } from 'react';
import { Zap, Clock, Database, Activity, FileCode, Network, Link2, Box, Waypoints, Sparkles, Gauge, CheckCircle2 } from 'lucide-react';
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { api, RepositoryOverview, SystemHealth } from '../../providers/api';
import { Card, StatCard, Badge, LoadingState, tooltipStyle } from '../ui';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { formatNumber, formatDuration, formatBytes } from '../../lib/format';

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
    return <LoadingState label="Loading workspace insights…" />;
  }

  const healthLabel = health.status === 'healthy' ? 'Healthy' : health.status === 'stale' ? 'Index Stale' : 'Not Indexed';
  const healthVariant = health.status === 'healthy' ? 'success' : health.status === 'stale' ? 'warning' : 'error';

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard
          label="Tokens Avoided"
          value={formatNumber(data.totalTokensAvoided)}
          icon={<Zap />}
          sublabel={`${data.contextCompressionPercent.toFixed(0)}% smaller than a manual read`}
          accent
        />
        <StatCard label="Est. Cost Saved" value={`$${(data.estCostSavedCents / 100).toFixed(2)}`} icon={<Sparkles />} sublabel={`${formatNumber(data.totalCalls)} tool calls total`} />
        <StatCard label="Tool Success Rate" value={`${data.successRate.toFixed(1)}%`} icon={<CheckCircle2 />} sublabel={data.failedCalls > 0 ? `${data.failedCalls} failed call(s)` : 'No failures logged'} />
        <StatCard label="Avg Response Time" value={formatDuration(data.avgLatencyMs)} icon={<Gauge />} sublabel="Per MCP tool call" />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <Card className="lg:col-span-2">
          <div className="flex items-center justify-between mb-6">
            <div>
              <h3 className="text-base font-medium text-[#fafafa]">Token Usage Over Time</h3>
              <p className="text-xs text-[#71717a] mt-0.5">Delivered to the agent vs. avoided, per hour</p>
            </div>
          </div>
          <div className="h-[280px]">
            {data.tokenUsageGraph.length === 0 ? (
              <div className="h-full flex items-center justify-center text-sm text-[#71717a]">No tool activity recorded yet</div>
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={data.tokenUsageGraph} margin={{ left: -12, right: 8 }}>
                  <defs>
                    <linearGradient id="colorUsed" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#71717a" stopOpacity={0.35} />
                      <stop offset="95%" stopColor="#71717a" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="colorSaved" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#ff6b35" stopOpacity={0.35} />
                      <stop offset="95%" stopColor="#ff6b35" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid vertical={false} stroke="#1f1f22" strokeDasharray="3 3" />
                  <XAxis dataKey="time" stroke="#52525b" fontSize={11} tickLine={false} axisLine={false} />
                  <YAxis stroke="#52525b" fontSize={11} tickLine={false} axisLine={false} tickFormatter={(v) => formatNumber(v)} width={44} />
                  <Tooltip {...tooltipStyle} formatter={(v: number, name: string) => [formatNumber(v) + ' tok', name === 'used' ? 'Delivered' : 'Avoided']} />
                  <Area type="monotone" dataKey="used" stroke="#71717a" strokeWidth={2} fillOpacity={1} fill="url(#colorUsed)" name="used" />
                  <Area type="monotone" dataKey="saved" stroke="#ff6b35" strokeWidth={2} fillOpacity={1} fill="url(#colorSaved)" name="saved" />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </div>
          <div className="flex items-center gap-5 mt-2 text-xs text-[#71717a]">
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-[#71717a]" /> Delivered to agent</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-[#ff6b35]" /> Avoided (real file-size baseline)</span>
          </div>
        </Card>

        <Card className="flex flex-col">
          <h3 className="text-base font-medium mb-6 text-[#fafafa]">System Health</h3>
          <div className="flex-1 space-y-5">
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Database className="w-4 h-4" /> Index Status</span>
              <Badge variant={healthVariant}>{healthLabel}</Badge>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Zap className="w-4 h-4" /> Cache Hit Rate</span>
              <span className="text-[#fafafa] font-medium text-sm">{health.cacheHitRate.toFixed(1)}%</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Clock className="w-4 h-4" /> Index Freshness</span>
              <span className="text-[#fafafa] font-medium text-sm">{formatDuration(health.indexFreshnessMs)} ago</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Activity className="w-4 h-4" /> Server Uptime</span>
              <span className="text-[#fafafa] font-medium text-sm">{formatDuration(health.uptimeMs)}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Box className="w-4 h-4" /> DB Size</span>
              <span className="text-[#fafafa] font-medium text-sm">{formatBytes(health.dbSizeBytes)}</span>
            </div>
          </div>
        </Card>
      </div>

      <div>
        <h3 className="text-lg font-semibold mb-4 text-[#fafafa]">Repository Snapshot</h3>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
          {[
            { label: 'Files', value: data.filesIndexed, icon: <FileCode className="w-3.5 h-3.5" /> },
            { label: 'Symbols', value: data.symbols, icon: <Box className="w-3.5 h-3.5" /> },
            { label: 'Edges', value: data.relationships, icon: <Link2 className="w-3.5 h-3.5" /> },
            { label: 'Communities', value: data.communities, icon: <Network className="w-3.5 h-3.5" /> },
            { label: 'Entrypoints', value: data.entrypoints, icon: <Waypoints className="w-3.5 h-3.5" /> },
            { label: 'Embeddings', value: data.embeddedSymbols, icon: <Database className="w-3.5 h-3.5" /> },
          ].map((stat) => (
            <div key={stat.label} className="bg-[#111113] p-4 rounded-xl border border-[#1f1f22] hover:border-[#27272a] transition-colors">
              <div className="text-[#71717a] text-xs mb-1.5 uppercase tracking-wider font-semibold flex items-center gap-1.5">
                {stat.icon} {stat.label}
              </div>
              <div className="text-2xl font-semibold text-[#fafafa]">{formatNumber(stat.value)}</div>
            </div>
          ))}
        </div>
        <p className="text-xs text-[#71717a] mt-3">{formatBytes(data.repositorySizeBytes)} of source on disk</p>
      </div>
    </div>
  );
}
