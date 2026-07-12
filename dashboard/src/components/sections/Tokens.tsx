import React, { useState, useEffect, useCallback } from 'react';
import { StatCard } from '../ui';
import { Activity, Coins, Percent, Zap } from 'lucide-react';
import { api, RepositoryOverview } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';

export function Tokens() {
  const [data, setData] = useState<RepositoryOverview | null>(null);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(() => {
    api.getOverview().then((overview) => {
      setData(overview);
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

  if (loading || !data) {
    return <div className="text-[#71717a] flex items-center justify-center h-full">Loading insights...</div>;
  }

  const formatNumber = (num: number) => {
    if (num >= 1000000) return (num / 1000000).toFixed(2) + 'M';
    if (num >= 1000) return (num / 1000).toFixed(1) + 'k';
    return num.toString();
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Token Intelligence</h2>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Tokens Avoided" value={formatNumber(data.tokensSaved)} icon={<Zap />} />
        <StatCard label="OpenAI Cost Saved" value={`$${(data.estCostSavedCents / 100).toFixed(2)}`} icon={<Coins />} />
        <StatCard label="Context Reduction" value={`${data.contextCompressionPercent.toFixed(1)}%`} icon={<Percent />} />
        <StatCard label="Graph Efficiency" value={`${data.relationshipResolutionPercent.toFixed(1)}%`} icon={<Activity />} />
      </div>
    </div>
  );
}
