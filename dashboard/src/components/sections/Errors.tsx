import React, { useEffect, useState, useCallback, useMemo } from 'react';
import { Card, SectionHeading, LoadingState, EmptyState, StatusDot } from '../ui';
import { XCircle, Terminal, CheckCircle2, Search } from 'lucide-react';
import { api, ErrorEvent } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { timeAgo } from '../../lib/format';

export function Errors() {
  const [errors, setErrors] = useState<ErrorEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState('');

  const loadData = useCallback(async () => {
    const data = await api.getErrors();
    setErrors(data);
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

  const filtered = useMemo(() => {
    if (!query.trim()) return errors;
    const q = query.toLowerCase();
    return errors.filter((e) => e.tool.toLowerCase().includes(q) || e.arguments.toLowerCase().includes(q));
  }, [errors, query]);

  if (loading) {
    return <LoadingState label="Loading failed tool calls…" />;
  }

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <SectionHeading
        title="Failed Tool Calls"
        subtitle="Every MCP call that returned an error, straight from the analytics log"
        action={
          <div className="relative">
            <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-[#71717a]" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter by tool or args…"
              className="bg-[#111113] border border-[#1f1f22] rounded-lg pl-9 pr-4 py-2 text-sm text-[#fafafa] focus:outline-none focus:border-[#ff6b35] transition-colors w-56"
            />
          </div>
        }
      />

      <div className="space-y-3">
        {filtered.length === 0 ? (
          <Card>
            <EmptyState
              icon={errors.length === 0 ? <CheckCircle2 className="w-8 h-8 text-[#22c55e]" /> : <Search className="w-8 h-8" />}
              title={errors.length === 0 ? 'No failures logged' : 'No matches'}
              subtitle={errors.length === 0 ? 'Every recorded MCP tool call has succeeded so far.' : 'Try a different filter term.'}
            />
          </Card>
        ) : (
          filtered.map((err) => (
            <Card key={err.id} className="!p-4 hover:border-[#ef4444]/30 transition-colors">
              <div className="flex flex-col md:flex-row gap-4 justify-between items-start md:items-center">
                <div className="flex items-start gap-4 min-w-0">
                  <div className="p-2 rounded-lg h-10 w-10 flex items-center justify-center shrink-0 bg-[#ef4444]/10 text-[#ef4444]">
                    <XCircle className="w-5 h-5" />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-3 mb-1">
                      <span className="font-semibold text-sm font-mono text-[#fafafa]">{err.tool}</span>
                      <span className="text-xs text-[#71717a] flex items-center gap-1">
                        <Terminal className="w-3 h-3" /> {err.latency_ms}ms
                      </span>
                    </div>
                    <p className="text-[#a1a1aa] text-sm font-mono mt-2 bg-[#09090b] p-2 rounded border border-[#1f1f22] break-all">
                      {err.arguments || 'No arguments'}
                    </p>
                  </div>
                </div>

                <div className="text-sm text-[#71717a] whitespace-nowrap flex items-center gap-2 shrink-0">
                  <StatusDot status="error" />
                  {timeAgo(err.timestamp)}
                </div>
              </div>
            </Card>
          ))
        )}
      </div>
    </div>
  );
}
