import React from 'react';
import { Card, StatCard } from '../ui';
import { Cpu, Zap, Activity, Coins, Percent } from 'lucide-react';
import { Area, AreaChart, Bar, BarChart, ResponsiveContainer, Tooltip, XAxis, YAxis, Cell } from 'recharts';

const savingsData = [
  { name: 'Mon', saved: 12000, used: 25000 },
  { name: 'Tue', saved: 15000, used: 22000 },
  { name: 'Wed', saved: 25000, used: 18000 },
  { name: 'Thu', saved: 18000, used: 30000 },
  { name: 'Fri', saved: 32000, used: 20000 },
  { name: 'Sat', saved: 45000, used: 15000 },
  { name: 'Sun', saved: 38000, used: 12000 },
];

const contextEfficiencyData = [
  { name: 'Week 1', ratio: 65 },
  { name: 'Week 2', ratio: 72 },
  { name: 'Week 3', ratio: 78 },
  { name: 'Week 4', ratio: 85 },
];

export function Tokens() {
  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Token Intelligence</h2>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        <StatCard label="Tokens Avoided" value="8.91M" icon={<Zap />} trend="18.3%" trendUp={true} />
        <StatCard label="OpenAI Cost Saved" value="$26.73" icon={<Coins />} trend="15.7%" trendUp={true} />
        <StatCard label="Context Reduction" value="85.2%" icon={<Percent />} trend="5.1%" trendUp={true} />
        <StatCard label="Graph Efficiency" value="94.1%" icon={<Activity />} trend="1.2%" trendUp={true} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <Card>
          <h3 className="text-lg font-medium mb-6">Weekly Savings Trend</h3>
          <div className="h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={savingsData}>
                <XAxis dataKey="name" stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} />
                <YAxis stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} tickFormatter={(v) => `${v/1000}k`} />
                <Tooltip 
                  cursor={{ fill: '#1f1f22' }}
                  contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                  itemStyle={{ color: '#fafafa' }}
                />
                <Bar dataKey="used" fill="#71717a" radius={[4, 4, 0, 0]} name="Tokens Used" stackId="a" />
                <Bar dataKey="saved" fill="#ff6b35" radius={[4, 4, 0, 0]} name="Tokens Saved" stackId="a" />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </Card>

        <Card>
          <h3 className="text-lg font-medium mb-6">Context Compression Ratio</h3>
          <div className="h-[300px]">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={contextEfficiencyData}>
                <defs>
                  <linearGradient id="colorRatio" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor="#22c55e" stopOpacity={0.3}/>
                    <stop offset="95%" stopColor="#22c55e" stopOpacity={0}/>
                  </linearGradient>
                </defs>
                <XAxis dataKey="name" stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} />
                <YAxis stroke="#71717a" fontSize={12} tickLine={false} axisLine={false} domain={[0, 100]} tickFormatter={(v) => `${v}%`} />
                <Tooltip 
                  contentStyle={{ backgroundColor: '#111113', borderColor: '#1f1f22', borderRadius: '8px' }}
                  itemStyle={{ color: '#fafafa' }}
                />
                <Area type="monotone" dataKey="ratio" stroke="#22c55e" strokeWidth={2} fillOpacity={1} fill="url(#colorRatio)" name="Compression Ratio" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </Card>
      </div>
    </div>
  );
}
