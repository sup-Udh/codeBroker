// API Provider definition

export interface RepositoryOverview {
  filesIndexed: number;
  symbols: number;
  relationships: number;
  communities: number;
  entrypoints: number;
  embeddings: number;
  languages: number;
  repositorySizeBytes: number;
  databaseSizeBytes: number;
  indexDurationMs: number;
  averageParseTimeMs: number;
  relationshipResolutionPercent: number;
  contextCompressionPercent: number;
  promptReductionPercent: number;
  tokensSaved: number;
  estCostSavedCents: number;
  estTimeSavedMs: number;
  avgAiContextSize: number;
  avgRetrievalTimeMs: number;
}

export interface GraphNode {
  id: string;
  type: string;
  label: string;
  size: number;
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  type: string;
}

export interface SystemHealth {
  databaseStatus: 'Healthy' | 'Warning' | 'Error';
  cacheHitRate: number;
  indexFreshnessMs: number;
  lastFullIndexMs: number;
  mcpServerStatus: 'Connected' | 'Disconnected';
  sqliteSizeMb: number;
}

export interface McpActivity {
  tool: string;
  prompt: string;
  success: boolean;
  latency_ms: number;
  tokens: number;
  cache_hit: boolean;
  timestamp: string;
}

export interface McpToolStats {
  name: string;
  calls: number;
  avg_latency: number;
  tokens_saved: number;
}

export interface CodeBrokerApi {
  getOverview: () => Promise<RepositoryOverview>;
  getSystemHealth: () => Promise<SystemHealth>;
  getGraph: () => Promise<{ nodes: GraphNode[], edges: GraphEdge[] }>;
  getMcpActivity: () => Promise<McpActivity[]>;
  getMcpTools: () => Promise<McpToolStats[]>;
}

export const api: CodeBrokerApi = {
  getOverview: async () => {
    try {
      const res = await fetch('/api/v1/stats/overview');
      const data = await res.json();
      return {
        filesIndexed: data.files_indexed || 0,
        symbols: data.symbols_indexed || 0,
        relationships: data.relationships_indexed || 0,
        communities: 0,
        entrypoints: 0,
        embeddings: 0,
        languages: 0,
        repositorySizeBytes: 0,
        databaseSizeBytes: 0,
        indexDurationMs: 0,
        averageParseTimeMs: 0,
        relationshipResolutionPercent: 0,
        contextCompressionPercent: 0,
        promptReductionPercent: 0,
        tokensSaved: data.total_tokens_avoided || 0,
        estCostSavedCents: ((data.total_tokens_avoided || 0) / 1000000) * 300,
        estTimeSavedMs: 0,
        avgAiContextSize: 0,
        avgRetrievalTimeMs: 0,
      };
    } catch (e) {
      console.error(e);
      throw e;
    }
  },
  getSystemHealth: async () => {
    try {
      const res = await fetch('/api/v1/stats/health');
      const data = await res.json();
      return {
        databaseStatus: 'Healthy',
        cacheHitRate: parseFloat(data.cache_hit_rate) || 0,
        indexFreshnessMs: 0,
        lastFullIndexMs: 0,
        mcpServerStatus: 'Connected',
        sqliteSizeMb: (data.db_size_bytes || 0) / (1024 * 1024),
      };
    } catch (e) {
      console.error(e);
      throw e;
    }
  },
  getGraph: async () => {
    try {
      const res = await fetch('/api/v1/graph/snapshot');
      return await res.json();
    } catch (e) {
      console.error(e);
      throw e;
    }
  },
  getMcpActivity: async () => {
    try {
      const res = await fetch('/api/v1/mcp/activity');
      return await res.json();
    } catch (e) {
      console.error(e);
      return [];
    }
  },
  getMcpTools: async () => {
    try {
      const res = await fetch('/api/v1/mcp/tools');
      return await res.json();
    } catch (e) {
      console.error(e);
      return [];
    }
  }
};
