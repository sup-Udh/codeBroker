import React, { useEffect, useState, useCallback } from 'react';
import { Card, Badge } from '../ui';
import { Clock, Code, Database, Search } from 'lucide-react';
import { api, McpActivity as McpActivityType } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';

function timeAgo(dateString: string) {
  const date = new Date(dateString);
  const now = new Date();
  const seconds = Math.floor((now.getTime() - date.getTime()) / 1000);
  
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
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
    return <div className="text-[#71717a] flex items-center justify-center h-full py-12">Loading activity...</div>;
  }

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Live AI Activity Timeline</h2>
        <div className="flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse"></span>
          <span className="text-sm text-[#71717a]">Listening for events</span>
        </div>
      </div>

      <div className="space-y-4">
        {activities.length === 0 ? (
          <div className="text-[#71717a] text-center py-8">No recent AI activity. Use a CodeBroker MCP tool to see it here!</div>
        ) : activities.map((activity, index) => {
          let promptDisplay = activity.prompt;
          if (promptDisplay && promptDisplay.length > 100) {
            promptDisplay = promptDisplay.substring(0, 100) + '...';
          }
          if (!promptDisplay || promptDisplay === 'null') {
            promptDisplay = 'No arguments provided';
          }

          return (
            <Card key={index} className="p-4 hover:border-[#ff6b35]/30 transition-colors cursor-pointer group">
              <div className="flex items-start justify-between">
                <div className="flex gap-4">
                  <div className="p-2 bg-[#1f1f22] rounded-lg h-10 w-10 flex items-center justify-center text-[#fafafa] group-hover:bg-[#ff6b35]/10 group-hover:text-[#ff6b35] transition-colors shrink-0">
                    {activity.tool.includes('search') ? <Search className="w-4 h-4" /> : 
                     activity.tool.includes('read') ? <Code className="w-4 h-4" /> : 
                     <Database className="w-4 h-4" />}
                  </div>
                  <div>
                    <div className="flex items-center gap-3 mb-1">
                      <span className="font-mono text-sm text-[#ff6b35]">{activity.tool}</span>
                      <Badge variant={activity.success ? 'success' : 'error'}>{activity.success ? 'Success' : 'Failed'}</Badge>
                      {activity.cache_hit && <Badge variant="default">Cache Hit</Badge>}
                    </div>
                    <p className="text-[#fafafa] font-medium text-sm break-all">"{promptDisplay}"</p>
                  </div>
                </div>
                
                <div className="flex flex-col items-end gap-2 text-sm text-[#71717a] shrink-0">
                  <div className="flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {timeAgo(activity.timestamp + 'Z')}
                  </div>
                  <div className="flex gap-3">
                    <span className="font-mono">{activity.latency_ms}ms</span>
                    <span>{activity.tokens.toLocaleString()} tkns</span>
                  </div>
                </div>
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
