import React, { useEffect, useState, useCallback } from 'react';
import { Card, StatCard, Badge, SectionHeading, LoadingState, EmptyState, CHART_COLORS, tooltipStyle } from '../ui';
import { FolderTree, Network, ShieldCheck, GitCompareArrows, CheckCircle2, Bug } from 'lucide-react';
import { ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, PieChart, Pie, Cell } from 'recharts';
import { api, CodebaseOverview } from '../../providers/api';
import { formatBytes, formatNumber } from '../../lib/format';

export function Codebase() {
  const [data, setData] = useState<CodebaseOverview | null>(null);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(() => {
    api.getCodebaseOverview().then((d) => {
      setData(d);
      setLoading(false);
    });
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  if (loading || !data) {
    return <LoadingState label="Analyzing repository structure…" />;
  }

  if (!data.available) {
    return (
      <div className="space-y-6 animate-in fade-in duration-500">
        <SectionHeading title="Repository Intelligence" subtitle="Structural health, hotspots, and language mix" />
        <Card>
          <EmptyState icon={<FolderTree className="w-8 h-8" />} title="No index found" subtitle="Run codebroker index in this workspace, then reload." />
        </Card>
      </div>
    );
  }

  const languagePie = data.languages.map((l) => ({ name: l.extension || 'other', value: l.percent }));
  const hotspotChart = data.hotspotFiles ?? [];

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <SectionHeading title="Repository Intelligence" subtitle="Structural health, hotspots, and language mix — computed from the live index" />

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Graph Health Score" value={`${data.healthScore}/100`} icon={<ShieldCheck />} sublabel="Penalized by cycles + orphans" accent />
        <StatCard label="Dependency Density" value={data.dependencyDensity.toFixed(2)} icon={<Network />} sublabel="Edges per symbol" />
        <StatCard
          label="Circular Dependencies"
          value={data.circularDependencies}
          icon={<GitCompareArrows />}
          sublabel="Cross-file cycles found"
        />
        <StatCard label="Orphan Symbols" value={`${data.orphanSymbolPercent.toFixed(1)}%`} icon={<Bug />} sublabel={`${formatNumber(data.orphanSymbols)} symbols with no edges`} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="flex flex-col">
          <h3 className="text-base font-medium mb-6 text-[#fafafa]">Language Distribution</h3>
          {data.languages.length === 0 ? (
            <EmptyState title="No language data yet" />
          ) : (
            <div className="flex-1 flex flex-col md:flex-row items-center justify-center gap-8">
              <div className="h-[220px] w-[220px] shrink-0">
                <ResponsiveContainer width="100%" height="100%">
                  <PieChart>
                    <Pie data={languagePie} cx="50%" cy="50%" innerRadius={55} outerRadius={92} paddingAngle={2} dataKey="value" stroke="none">
                      {languagePie.map((_, index) => (
                        <Cell key={`cell-${index}`} fill={CHART_COLORS[index % CHART_COLORS.length]} />
                      ))}
                    </Pie>
                    <Tooltip {...tooltipStyle} formatter={(value: number) => `${value.toFixed(1)}%`} />
                  </PieChart>
                </ResponsiveContainer>
              </div>
              <div className="flex-1 space-y-3 w-full">
                {data.languages.map((item, i) => (
                  <div key={item.extension} className="flex items-center justify-between text-sm">
                    <div className="flex items-center gap-2">
                      <span className="w-3 h-3 rounded-sm shrink-0" style={{ backgroundColor: CHART_COLORS[i % CHART_COLORS.length] }} />
                      <span className="text-[#fafafa] font-mono">.{item.extension}</span>
                    </div>
                    <span className="text-[#71717a] font-mono text-xs">{item.percent.toFixed(1)}% · {item.files} files</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </Card>

        <Card>
          <h3 className="text-base font-medium mb-1 text-[#fafafa]">Architectural Hotspots</h3>
          <p className="text-xs text-[#71717a] mb-6">Files ranked by real incoming + outgoing edge weight</p>
          <div className="h-[250px]">
            {hotspotChart.length === 0 ? (
              <EmptyState title="No hotspots detected" />
            ) : (
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={hotspotChart} layout="vertical" margin={{ top: 0, right: 16, left: 0, bottom: 0 }}>
                  <XAxis type="number" stroke="#52525b" fontSize={11} tickLine={false} axisLine={false} />
                  <YAxis
                    type="category"
                    dataKey="file_path"
                    stroke="#a1a1aa"
                    fontSize={11}
                    tickLine={false}
                    axisLine={false}
                    width={160}
                    tickFormatter={(v: string) => (v.length > 26 ? '…' + v.slice(-25) : v)}
                  />
                  <Tooltip {...tooltipStyle} cursor={{ fill: '#1f1f22' }} formatter={(v: number) => [v, 'Aggregate score']} labelFormatter={(l) => l} />
                  <Bar dataKey="aggregate_score" fill="#ff6b35" radius={[0, 4, 4, 0]} barSize={16} />
                </BarChart>
              </ResponsiveContainer>
            )}
          </div>
        </Card>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <h3 className="text-base font-medium mb-4 text-[#fafafa]">Largest Directories</h3>
          {data.directories.length === 0 ? (
            <EmptyState title="No directory data yet" />
          ) : (
            <div className="space-y-3">
              {data.directories.map((dir) => (
                <div key={dir.path} className="flex items-center justify-between p-3 bg-[#09090b] rounded-lg border border-[#1f1f22]">
                  <div className="flex items-center gap-3 min-w-0">
                    <FolderTree className="w-4 h-4 text-[#ff6b35] shrink-0" />
                    <span className="font-mono text-sm truncate" title={dir.path}>{dir.path}</span>
                  </div>
                  <div className="flex items-center gap-4 text-xs text-[#71717a] shrink-0">
                    <span>{dir.files} files</span>
                    <span className="font-mono">{formatBytes(dir.bytes)}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>

        <Card>
          <h3 className="text-base font-medium mb-4 text-[#fafafa]">Circular Dependencies</h3>
          {data.cycleExamples.length === 0 ? (
            <div className="flex items-center gap-3 p-4 bg-[#22c55e]/5 border border-[#22c55e]/20 rounded-lg text-sm text-[#22c55e]">
              <CheckCircle2 className="w-4 h-4 shrink-0" />
              No circular dependencies detected in the current index.
            </div>
          ) : (
            <div className="space-y-3">
              {data.cycleExamples.map((cycle, i) => (
                <div key={i} className="p-3 bg-[#09090b] rounded-lg border border-[#1f1f22]">
                  <div className="flex items-center gap-2 mb-2">
                    <Badge variant={cycle.cross_file ? 'error' : 'warning'}>{cycle.cross_file ? 'Cross-file' : 'Same-file'}</Badge>
                    <span className="text-xs text-[#71717a]">{cycle.length} nodes</span>
                  </div>
                  <div className="flex items-center gap-1 flex-wrap text-xs font-mono text-[#a1a1aa]">
                    {cycle.nodes.map((n, idx) => (
                      <React.Fragment key={idx}>
                        <span className="px-2 py-0.5 bg-[#1f1f22] rounded">{n.name}</span>
                        {idx < cycle.nodes.length - 1 && <span className="text-[#71717a]">→</span>}
                      </React.Fragment>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
