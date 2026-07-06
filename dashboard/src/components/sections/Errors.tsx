import React from 'react';
import { Card, Badge } from '../ui';
import { AlertTriangle, Terminal, XCircle, Search } from 'lucide-react';

const errorLogs = [
  { id: 1, type: 'MCP Failure', message: 'Context payload exceeded 128k token limit', time: '10 mins ago', source: 'impact_analysis', severity: 'error' },
  { id: 2, type: 'Index Warning', message: 'Failed to parse recursive AST in complex_types.rs', time: '1 hour ago', source: 'tree-sitter', severity: 'warning' },
  { id: 3, type: 'Embedding Error', message: 'OpenAI API timeout (504 Gateway Time-out)', time: '2 hours ago', source: 'semantic_search', severity: 'error' },
  { id: 4, type: 'Graph Broken Edge', message: 'Dangling reference to deleted symbol "AuthContext"', time: '5 hours ago', source: 'linker', severity: 'warning' },
];

export function Errors() {
  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">System Diagnostics & Errors</h2>
        <div className="relative">
          <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-[#71717a]" />
          <input 
            type="text" 
            placeholder="Search logs..." 
            className="bg-[#111113] border border-[#1f1f22] rounded-lg pl-9 pr-4 py-2 text-sm text-[#fafafa] focus:outline-none focus:border-[#ff6b35] transition-colors"
          />
        </div>
      </div>

      <div className="space-y-4">
        {errorLogs.map((log) => (
          <Card key={log.id} className="p-4 hover:border-[#1f1f22] transition-colors flex flex-col md:flex-row gap-4 justify-between items-start md:items-center">
            <div className="flex items-start gap-4">
              <div className={`p-2 rounded-lg h-10 w-10 flex items-center justify-center shrink-0 ${
                log.severity === 'error' ? 'bg-red-500/10 text-red-500' : 'bg-yellow-500/10 text-yellow-500'
              }`}>
                {log.severity === 'error' ? <XCircle className="w-5 h-5" /> : <AlertTriangle className="w-5 h-5" />}
              </div>
              <div>
                <div className="flex items-center gap-3 mb-1">
                  <span className="font-semibold text-sm">{log.type}</span>
                  <Badge variant={log.severity as 'error' | 'warning'}>{log.severity}</Badge>
                  <span className="text-xs text-[#71717a] flex items-center gap-1">
                    <Terminal className="w-3 h-3" /> {log.source}
                  </span>
                </div>
                <p className="text-[#fafafa] text-sm font-mono mt-2 bg-[#09090b] p-2 rounded border border-[#1f1f22] inline-block">
                  {log.message}
                </p>
              </div>
            </div>
            
            <div className="text-sm text-[#71717a] whitespace-nowrap">
              {log.time}
            </div>
          </Card>
        ))}
        {errorLogs.length === 0 && (
          <div className="text-center p-12 text-[#71717a]">
            <AlertTriangle className="w-8 h-8 mx-auto mb-4 opacity-50" />
            <p>No recent errors found.</p>
          </div>
        )}
      </div>
    </div>
  );
}
