import React from 'react';
import { Card, StatCard, Badge } from '../ui';
import { FolderTree, Code2, Network, ShieldCheck, ListTree } from 'lucide-react';
import { ResponsiveContainer, BarChart, Bar, XAxis, YAxis, Tooltip, PieChart, Pie, Cell } from 'recharts';

const languageData = [
  { name: 'TypeScript', value: 45.2 },
  { name: 'TSX', value: 28.7 },
  { name: 'JavaScript', value: 12.3 },
  { name: 'JSON', value: 6.8 },
  { name: 'CSS', value: 3.2 },
  { name: 'Others', value: 3.8 },
];

const COLORS = ['#ff6b35', '#22c55e', '#3b82f6', '#a855f7', '#ec4899', '#71717a'];

const complexityData = [
  { name: 'Auth', score: 85 },
  { name: 'Database', score: 62 },
  { name: 'UI Components', score: 45 },
  { name: 'API Routes', score: 78 },
  { name: 'Utils', score: 20 },
];

export function Codebase() {
  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Repository Intelligence</h2>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Graph Health Score" value="98/100" icon={<ShieldCheck />} />
        <StatCard label="Index Completeness" value="100%" icon={<ListTree />} />
        <StatCard label="Dependency Density" value="2.4" icon={<Network />} trend="0.1" trendUp={false} />
        <StatCard label="Circular Dependencies" value="0" icon={<FolderTree />} trend="0" trendUp={true} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card className="flex flex-col">
          <h3 className="text-lg font-medium mb-6">Language Distribution</h3>
          <div className="flex-1 flex flex-col md:flex-row items-center justify-center gap-8">
            <div className="h-[250px] w-[250px]">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    data={languageData}
                    cx="50%"
                    cy="50%"
                    innerRadius={60}
                    outerRadius={100}
                    paddingAngle={2}
                    dataKey="value"
                    stroke="none"
                  >
                    {languageData.map((entry, index) => (
                      <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip 
                    contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                    itemStyle={{ color: '#fafafa' }}
                    formatter={(value) => `${value}%`}
                  />
                </PieChart>
              </ResponsiveContainer>
            </div>
            <div className="flex-1 space-y-3 w-full">
              {languageData.map((item, i) => (
                <div key={item.name} className="flex items-center justify-between text-sm">
                  <div className="flex items-center gap-2">
                    <span className="w-3 h-3 rounded-sm" style={{ backgroundColor: COLORS[i] }}></span>
                    <span className="text-[#fafafa] font-medium">{item.name}</span>
                  </div>
                  <span className="text-[#71717a] font-mono">{item.value}%</span>
                </div>
              ))}
            </div>
          </div>
        </Card>

        <Card>
          <h3 className="text-lg font-medium mb-6">Subsystem Complexity Score</h3>
          <div className="h-[250px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={complexityData} layout="vertical" margin={{ top: 0, right: 0, left: 40, bottom: 0 }}>
                <XAxis type="number" domain={[0, 100]} stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} />
                <YAxis type="category" dataKey="name" stroke="#fafafa" fontSize={12} tickLine={false} axisLine={false} />
                <Tooltip 
                  cursor={{ fill: '#1f1f22' }}
                  contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                  itemStyle={{ color: '#fafafa' }}
                />
                <Bar dataKey="score" fill="#ff6b35" radius={[0, 4, 4, 0]} barSize={20} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <div className="flex justify-between items-center mb-4">
            <h3 className="text-lg font-medium">Largest Directories</h3>
          </div>
          <div className="space-y-4">
            {[
              { path: 'src/components', size: '1.2 MB', files: 45 },
              { path: 'src/lib/core', size: '840 KB', files: 12 },
              { path: 'src/api/routes', size: '420 KB', files: 24 },
              { path: 'src/utils', size: '150 KB', files: 18 },
            ].map((dir) => (
              <div key={dir.path} className="flex items-center justify-between p-3 bg-[#09090b] rounded-lg border border-[#1f1f22]">
                <div className="flex items-center gap-3">
                  <FolderTree className="w-4 h-4 text-[#ff6b35]" />
                  <span className="font-mono text-sm">{dir.path}</span>
                </div>
                <div className="flex items-center gap-4 text-sm text-[#71717a]">
                  <span>{dir.files} files</span>
                  <span className="font-mono">{dir.size}</span>
                </div>
              </div>
            ))}
          </div>
        </Card>

        <Card>
          <div className="flex justify-between items-center mb-4">
            <h3 className="text-lg font-medium">Architectural Hotspots</h3>
          </div>
          <div className="space-y-4">
            {[
              { path: 'src/lib/core/engine.ts', links: 124, type: 'High Coupling' },
              { path: 'src/api/context.ts', links: 89, type: 'Central Hub' },
              { path: 'src/db/connection.ts', links: 56, type: 'Bottleneck' },
              { path: 'src/utils/parser.ts', links: 42, type: 'Utility' },
            ].map((hotspot) => (
              <div key={hotspot.path} className="flex items-center justify-between p-3 bg-[#09090b] rounded-lg border border-[#1f1f22]">
                <div className="flex items-center gap-3">
                  <Code2 className="w-4 h-4 text-[#22c55e]" />
                  <span className="font-mono text-sm">{hotspot.path}</span>
                </div>
                <div className="flex items-center gap-4 text-sm text-[#71717a]">
                  <Badge variant={hotspot.links > 100 ? 'error' : hotspot.links > 50 ? 'warning' : 'default'}>{hotspot.type}</Badge>
                  <span className="font-mono">{hotspot.links} edges</span>
                </div>
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}
