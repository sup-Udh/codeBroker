import React, { useEffect, useState, useCallback, useMemo } from 'react';
import { Card, SectionHeading, LoadingState, EmptyState, CHART_COLORS, tooltipStyle } from '../ui';
import { ResponsiveContainer, PieChart, Pie, Cell, Tooltip } from 'recharts';
import { Wrench } from 'lucide-react';
import { api, McpToolStats } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { formatNumber } from '../../lib/format';

export function Tools() {
  const [tools, setTools] = useState<McpToolStats[]>([]);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(async () => {
    const data = await api.getMcpTools();
    setTools(data);
    setLoading(false);
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  useLiveEvents((type) => {
    if (type === 'mcp_activity') {
      loadData();
    }
  });

  const totalCalls = useMemo(() => tools.reduce((acc, t) => acc + t.calls, 0), [tools]);

  const toolUsageData = useMemo(
    () =>
      tools.map((t) => ({
        name: t.name,
        calls: t.calls,
        percentage: totalCalls > 0 ? Math.round((t.calls / totalCalls) * 100) : 0,
        avg_latency: t.avg_latency,
        failures: t.failures,
        tokens_saved: t.tokens_saved,
      })),
    [tools, totalCalls]
  );

  const maxLatency = useMemo(() => Math.max(1, ...toolUsageData.map((t) => t.avg_latency)), [toolUsageData]);

  if (loading) {
    return <LoadingState label="Loading tool analytics…" />;
  }

  if (tools.length === 0) {
    return (
      <div className="space-y-6 animate-in fade-in duration-500">
        <SectionHeading title="Tool Analytics" subtitle="Usage and efficiency per CodeBroker tool" />
        <Card>
          <EmptyState icon={<Wrench className="w-8 h-8" />} title="No tool calls yet" subtitle="Stats appear here as soon as an agent calls a CodeBroker MCP tool." />
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <SectionHeading title="Tool Analytics" subtitle="Usage and efficiency per CodeBroker tool" />

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="flex flex-col h-full">
          <h3 className="text-base font-medium mb-6 text-[#fafafa]">Call Distribution</h3>
          <div className="flex-1 flex flex-col xl:flex-row items-center justify-center gap-8">
            <div className="h-[200px] w-[200px] relative shrink-0">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie data={toolUsageData} cx="50%" cy="50%" innerRadius={62} outerRadius={82} paddingAngle={3} dataKey="calls" stroke="none">
                    {toolUsageData.map((_, index) => (
                      <Cell key={`cell-${index}`} fill={CHART_COLORS[index % CHART_COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip {...tooltipStyle} formatter={(v: number, _n, p: any) => [`${v} calls (${p.payload.percentage}%)`, p.payload.name]} />
                </PieChart>
              </ResponsiveContainer>
              <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
                <span className="text-2xl font-bold text-[#fafafa]">{totalCalls.toLocaleString()}</span>
                <span className="text-xs text-[#71717a]">Total Calls</span>
              </div>
            </div>
            <div className="flex-1 space-y-3 w-full">
              {toolUsageData.map((item, i) => (
                <div key={item.name} className="flex items-center justify-between text-sm gap-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: CHART_COLORS[i % CHART_COLORS.length] }} />
                    <span className="text-[#fafafa] font-mono truncate" title={item.name}>{item.name}</span>
                  </div>
                  <div className="flex items-center gap-3 shrink-0 text-xs">
                    <span className="text-[#71717a] w-9 text-right">{item.percentage}%</span>
                    <span className="text-[#fafafa] font-medium w-8 text-right">{item.calls}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </Card>

        <Card className="flex flex-col h-full">
          <h3 className="text-base font-medium mb-6 text-[#fafafa]">Average Latency &amp; Reliability</h3>
          <div className="flex-1 flex flex-col justify-center space-y-4">
            {toolUsageData.map((item, i) => (
              <div key={item.name} className="flex flex-col gap-2">
                <div className="flex justify-between text-sm gap-2">
                  <span className="font-mono text-[#a1a1aa] truncate">{item.name}</span>
                  <div className="flex items-center gap-2 shrink-0">
                    {item.failures > 0 && <span className="text-xs text-[#ef4444]">{item.failures} failed</span>}
                    <span className="font-medium text-[#fafafa] text-xs">{item.avg_latency}ms</span>
                  </div>
                </div>
                <div className="w-full bg-[#1f1f22] h-1.5 rounded-full overflow-hidden">
                  <div
                    className="h-full rounded-full transition-all duration-500"
                    style={{ backgroundColor: CHART_COLORS[i % CHART_COLORS.length], width: `${Math.max(2, (item.avg_latency / maxLatency) * 100)}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>

      <Card>
        <h3 className="text-base font-medium mb-4 text-[#fafafa]">Per-Tool Breakdown</h3>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-[#71717a] uppercase tracking-wider border-b border-[#1f1f22]">
                <th className="pb-3 font-semibold">Tool</th>
                <th className="pb-3 font-semibold text-right">Calls</th>
                <th className="pb-3 font-semibold text-right">Avg Latency</th>
                <th className="pb-3 font-semibold text-right">Tokens Avoided</th>
                <th className="pb-3 font-semibold text-right">Failures</th>
              </tr>
            </thead>
            <tbody>
              {toolUsageData.map((item) => (
                <tr key={item.name} className="border-b border-[#1f1f22] last:border-0">
                  <td className="py-3 font-mono text-[#fafafa]">{item.name}</td>
                  <td className="py-3 text-right text-[#a1a1aa]">{item.calls}</td>
                  <td className="py-3 text-right text-[#a1a1aa]">{item.avg_latency}ms</td>
                  <td className="py-3 text-right text-[#22c55e]">{formatNumber(item.tokens_saved)}</td>
                  <td className="py-3 text-right">{item.failures > 0 ? <span className="text-[#ef4444]">{item.failures}</span> : <span className="text-[#71717a]">0</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}
