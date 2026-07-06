import React, { useEffect, useState, useCallback, useMemo } from 'react';
import { Card } from '../ui';
import { ResponsiveContainer, PieChart, Pie, Cell, Tooltip } from 'recharts';
import { api, McpToolStats } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';

const COLORS = ['#ff6b35', '#22c55e', '#3b82f6', '#a855f7', '#ec4899', '#eab308', '#71717a'];

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

  const toolUsageData = useMemo(() => {
    return tools.map((t) => ({
      name: t.name,
      calls: t.calls,
      percentage: totalCalls > 0 ? Math.round((t.calls / totalCalls) * 100) : 0,
      avg_latency: t.avg_latency,
    }));
  }, [tools, totalCalls]);

  if (loading) {
    return <div className="text-[#71717a] flex items-center justify-center h-full py-12">Loading tools...</div>;
  }

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Tool Analytics</h2>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="flex flex-col h-full">
          <h3 className="text-lg font-medium mb-6">MCP Tool Usage</h3>
          <div className="flex-1 flex flex-col xl:flex-row items-center justify-center gap-8">
            <div className="h-[200px] w-[200px] relative shrink-0">
              {totalCalls > 0 ? (
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie
                      data={toolUsageData}
                      cx="50%"
                      cy="50%"
                      innerRadius={60}
                      outerRadius={80}
                      paddingAngle={5}
                      dataKey="calls"
                      stroke="none"
                    >
                      {toolUsageData.map((entry, index) => (
                        <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip 
                      contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                      itemStyle={{ color: '#fafafa' }}
                    />
                  </PieChart>
                </ResponsiveContainer>
              ) : (
                <div className="w-full h-full rounded-full border-[20px] border-[#1f1f22] flex items-center justify-center"></div>
              )}
              <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
                <span className="text-2xl font-bold">{totalCalls.toLocaleString()}</span>
                <span className="text-xs text-[#71717a]">Total Calls</span>
              </div>
            </div>
            <div className="flex-1 space-y-3 w-full">
              {toolUsageData.map((item, i) => (
                <div key={item.name} className="flex flex-col sm:flex-row sm:items-center justify-between text-sm gap-2">
                  <div className="flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: COLORS[i % COLORS.length] }}></span>
                    <span className="text-[#fafafa] font-mono truncate max-w-[200px]" title={item.name}>{item.name}</span>
                  </div>
                  <div className="flex items-center gap-4 shrink-0">
                    <span className="text-[#71717a] w-8 text-right">{item.percentage}%</span>
                    <span className="text-[#fafafa] font-medium w-8 text-right">{item.calls}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </Card>

        <Card className="flex flex-col h-full">
          <h3 className="text-lg font-medium mb-6">Average Latency per Tool</h3>
          <div className="flex-1 flex flex-col justify-center space-y-4">
            {toolUsageData.map((item, i) => (
              <div key={item.name} className="flex flex-col gap-2">
                <div className="flex justify-between text-sm">
                  <span className="font-mono text-[#71717a]">{item.name}</span>
                  <span className="font-medium text-[#fafafa]">{item.avg_latency}ms</span>
                </div>
                <div className="w-full bg-[#1f1f22] h-2 rounded-full overflow-hidden">
                  <div 
                    className="h-full rounded-full"
                    style={{ 
                      backgroundColor: COLORS[i % COLORS.length],
                      width: `${Math.min(100, Math.max(2, (item.avg_latency / 5000) * 100))}%`
                    }}
                  />
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}
