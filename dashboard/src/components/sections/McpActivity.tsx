import React, { useEffect, useState, useCallback } from 'react';
import { Card, Badge, SectionHeading, LoadingState, EmptyState, StatusDot } from '../ui';
import { Clock, Code, Database, Search, GitBranch, Network, Radio } from 'lucide-react';
import { api, McpActivity as McpActivityType } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { timeAgo, formatNumber } from '../../lib/format';

function toolIcon(tool: string) {
  if (tool.includes('search')) return <Search className="w-4 h-4" />;
  if (tool.includes('read')) return <Code className="w-4 h-4" />;
  if (tool.includes('graph') || tool.includes('path')) return <Network className="w-4 h-4" />;
  if (tool.includes('duplicate') || tool.includes('cycle') || tool.includes('hotspot')) return <GitBranch className="w-4 h-4" />;
  return <Database className="w-4 h-4" />;
}

export function McpActivity() {
  const [activities, setActivities] = useState<McpActivityType[]>([]);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(async () => {
    const data = await api.getMcpActivity();
    setActivities(data);
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

  if (loading) {
    return <LoadingState label="Loading activity…" />;
  }

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <SectionHeading
        title="Live AI Activity Timeline"
        subtitle="Every MCP tool call, most recent first"
        action={
          <div className="flex items-center gap-2 text-sm text-[#71717a]">
            <StatusDot status="success" />
            Listening for events
          </div>
        }
      />

      <div className="space-y-3">
        {activities.length === 0 ? (
          <Card>
            <EmptyState icon={<Radio className="w-8 h-8" />} title="No recent AI activity" subtitle="Use a CodeBroker MCP tool from your agent to see it appear here in real time." />
          </Card>
        ) : (
          activities.map((activity, index) => {
            let promptDisplay = activity.prompt;
            if (promptDisplay && promptDisplay.length > 120) {
              promptDisplay = promptDisplay.substring(0, 120) + '…';
            }
            if (!promptDisplay || promptDisplay === 'null' || promptDisplay === '{}') {
              promptDisplay = 'No arguments provided';
            }

            return (
              <Card key={index} className="!p-4 hover:border-[#ff6b35]/30 transition-colors group">
                <div className="flex items-start justify-between gap-4">
                  <div className="flex gap-4 min-w-0">
                    <div className="p-2 bg-[#1f1f22] rounded-lg h-10 w-10 flex items-center justify-center text-[#fafafa] group-hover:bg-[#ff6b35]/10 group-hover:text-[#ff6b35] transition-colors shrink-0">
                      {toolIcon(activity.tool)}
                    </div>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2 mb-1 flex-wrap">
                        <span className="font-mono text-sm text-[#ff6b35]">{activity.tool}</span>
                        <Badge variant={activity.success ? 'success' : 'error'}>{activity.success ? 'Success' : 'Failed'}</Badge>
                        {activity.cache_hit && <Badge variant="info">Cache Hit</Badge>}
                      </div>
                      <p className="text-[#a1a1aa] text-sm break-all font-mono">{promptDisplay}</p>
                    </div>
                  </div>

                  <div className="flex flex-col items-end gap-2 text-sm text-[#71717a] shrink-0">
                    <div className="flex items-center gap-1 text-xs">
                      <Clock className="w-3 h-3" />
                      {timeAgo(activity.timestamp)}
                    </div>
                    <div className="flex gap-3 text-xs">
                      <span className="font-mono">{activity.latency_ms}ms</span>
                      <span>{formatNumber(activity.tokens)} tok</span>
                      {activity.tokens_saved > 0 && <span className="text-[#22c55e]">-{formatNumber(activity.tokens_saved)}</span>}
                    </div>
                  </div>
                </div>
              </Card>
            );
          })
        )}
      </div>
    </div>
  );
}
