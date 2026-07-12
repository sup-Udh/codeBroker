import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { StatCard, Card, SectionHeading, LoadingState, tooltipStyle } from '../ui';
import { Coins, Percent, Zap, Cpu, FileStack, HardDrive } from 'lucide-react';
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { api, RepositoryOverview, McpToolStats } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { formatNumber } from '../../lib/format';

export function Tokens() {
  const [data, setData] = useState<RepositoryOverview | null>(null);
  const [tools, setTools] = useState<McpToolStats[]>([]);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(() => {
    Promise.all([api.getOverview(), api.getMcpTools()]).then(([overview, toolStats]) => {
      setData(overview);
      setTools(toolStats);
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

  const toolChartData = useMemo(
    () =>
      tools
        .filter((t) => t.tokens_saved > 0)
        .sort((a, b) => b.tokens_saved - a.tokens_saved)
        .slice(0, 8)
        .map((t) => ({ name: t.name, saved: t.tokens_saved })),
    [tools]
  );

  if (loading || !data) {
    return <LoadingState label="Loading token economics…" />;
  }

  const avgSavedPerCall = data.totalCalls > 0 ? data.totalTokensAvoided / data.totalCalls : 0;

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <SectionHeading title="Token Economics" subtitle="What CodeBroker actually kept out of the model's context window" />

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Tokens Avoided" value={formatNumber(data.totalTokensAvoided)} icon={<Zap />} accent sublabel="vs. reading the same files in full" />
        <StatCard label="Tokens Delivered" value={formatNumber(data.totalTokensUsed)} icon={<Cpu />} sublabel="Actually sent to the model" />
        <StatCard label="Raw File Baseline" value={formatNumber(data.totalRawTokens)} icon={<HardDrive />} sublabel="Real bytes of files each answer touched" />
        <StatCard label="Context Reduction" value={`${data.contextCompressionPercent.toFixed(1)}%`} icon={<Percent />} sublabel="Delivered vs. raw baseline" />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <Card className="lg:col-span-2">
          <h3 className="text-base font-medium mb-1 text-[#fafafa]">Tokens Avoided by Tool</h3>
          <p className="text-xs text-[#71717a] mb-6">Top tools ranked by real tokens kept out of context</p>
          <div className="h-[260px]">
            {toolChartData.length === 0 ? (
              <div className="h-full flex items-center justify-center text-sm text-[#71717a]">No tool activity recorded yet</div>
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={toolChartData} layout="vertical" margin={{ top: 0, right: 16, left: 8, bottom: 0 }}>
                  <CartesianGrid horizontal={false} stroke="#1f1f22" strokeDasharray="3 3" />
                  <XAxis type="number" stroke="#52525b" fontSize={11} tickLine={false} axisLine={false} tickFormatter={(v) => formatNumber(v)} />
                  <YAxis type="category" dataKey="name" stroke="#a1a1aa" fontSize={12} tickLine={false} axisLine={false} width={140} />
                  <Tooltip {...tooltipStyle} cursor={{ fill: '#1f1f22' }} formatter={(v: number) => [formatNumber(v) + ' tokens', 'Avoided']} />
                  <Bar dataKey="saved" fill="#ff6b35" radius={[0, 4, 4, 0]} barSize={16} />
                </BarChart>
              </ResponsiveContainer>
            )}
          </div>
        </Card>

        <Card className="flex flex-col">
          <h3 className="text-base font-medium mb-6 text-[#fafafa]">Per-Call Average</h3>
          <div className="flex-1 space-y-5">
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Coins className="w-4 h-4" /> Cost Saved</span>
              <span className="text-[#fafafa] font-medium text-sm">${(data.estCostSavedCents / 100).toFixed(2)}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><FileStack className="w-4 h-4" /> Total Calls</span>
              <span className="text-[#fafafa] font-medium text-sm">{formatNumber(data.totalCalls)}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[#71717a] flex items-center gap-2 text-sm"><Zap className="w-4 h-4" /> Avg Avoided / Call</span>
              <span className="text-[#fafafa] font-medium text-sm">{formatNumber(avgSavedPerCall)}</span>
            </div>
          </div>
        </Card>
      </div>

      <Card className="!p-5">
        <h4 className="text-sm font-medium text-[#fafafa] mb-2">How this is measured</h4>
        <p className="text-sm text-[#a1a1aa] leading-relaxed">
          The baseline isn't a flat per-call estimate — it's the real on-disk byte size of the
          exact files each answer actually named (callers, callees, matched files, hotspot
          files, ...), converted to tokens. Single-file tools (skeletons, snippets) compare
          against that one file's real size; repo-wide tools (<code className="text-[#ff6b35]">repository_stats</code>)
          compare against every indexed file in scope. "Tokens Delivered" is a blended
          char+word estimate of the actual response text, closer to real tokenizer behavior
          than a flat characters-per-token ratio.
        </p>
      </Card>
    </div>
  );
}
