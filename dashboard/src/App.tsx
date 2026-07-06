import React, { useState } from 'react';
import { 
  LayoutDashboard, 
  Coins, 
  Activity, 
  FolderTree, 
  Network, 
  Wrench, 
  AlertCircle, 
  Settings 
} from 'lucide-react';
import { Sidebar } from './components/layout/Sidebar';
import { Overview } from './components/sections/Overview';
import { Tokens } from './components/sections/Tokens';
import { McpActivity } from './components/sections/McpActivity';
import { Codebase } from './components/sections/Codebase';
import { SemanticGraph } from './components/sections/SemanticGraph';
import { Tools } from './components/sections/Tools';
import { Errors } from './components/sections/Errors';

export type Section = 'overview' | 'tokens' | 'activity' | 'codebase' | 'graph' | 'tools' | 'errors' | 'settings';

function App() {
  const [activeSection, setActiveSection] = useState<Section>('overview');

  const renderSection = () => {
    switch (activeSection) {
      case 'overview': return <Overview />;
      case 'tokens': return <Tokens />;
      case 'activity': return <McpActivity />;
      case 'codebase': return <Codebase />;
      case 'graph': return <SemanticGraph />;
      case 'tools': return <Tools />;
      case 'errors': return <Errors />;
      default: return <Overview />;
    }
  };

  return (
    <div className="flex h-screen bg-[#09090b] text-[#fafafa] font-sans overflow-hidden">
      <Sidebar activeSection={activeSection} onSelect={setActiveSection} />
      <main className="flex-1 flex flex-col min-w-0 overflow-y-auto">
        <header className="h-16 border-b border-[#1f1f22] flex items-center px-8 justify-between flex-shrink-0 bg-[#09090b]/80 backdrop-blur-sm sticky top-0 z-10">
          <div className="flex items-center gap-4">
            <h2 className="text-lg font-semibold tracking-tight capitalize">
              {activeSection === 'activity' ? 'MCP Activity' : activeSection}
            </h2>
            <span className="text-sm text-[#71717a]">Real-time insights into your codebase and AI development metrics</span>
          </div>
          <div className="flex items-center gap-4 text-sm">
            <div className="flex items-center gap-2 text-[#71717a] bg-[#111113] px-3 py-1.5 rounded-full border border-[#1f1f22]">
              <span className="w-2 h-2 rounded-full bg-green-500"></span>
              MCP Server Connected
            </div>
            <div className="text-[#71717a]">
              Last 24 hours
            </div>
          </div>
        </header>
        <div className="p-8">
          {renderSection()}
        </div>
      </main>
    </div>
  );
}

export default App;
