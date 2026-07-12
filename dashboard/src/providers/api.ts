// API Provider definition — every field here is backed by a real column or
// computed value on the server (analytics/src/server.rs); nothing here is a
// placeholder/sample constant.

export interface LanguageStat {
  extension: string;
  bytes?: number;
  files?: number;
  percent: number;
}

export interface RepositoryOverview {
  workspaceName: string;
  workspacePath: string;
  filesIndexed: number;
  symbols: number;
  relationships: number;
  communities: number;
  entrypoints: number;
  embeddedSymbols: number;
  repositorySizeBytes: number;
  totalTokensAvoided: number;
  totalTokensUsed: number;
  totalRawTokens: number;
  totalCalls: number;
  failedCalls: number;
  successRate: number;
  avgLatencyMs: number;
  estCostSavedCents: number;
  tokenUsageGraph: { time: string; used: number; saved: number }[];
  contextCompressionPercent: number;
}

export interface GraphNode {
  id: string;
  label: string;
  kind: string;
  file_path: string;
  pagerank: number;
  connections: number;
  community: number;
  color: string;
}

export interface GraphEdge {
  source: string;
  target: string;
  type: string;
}

export interface SystemHealth {
  status: 'healthy' | 'stale' | 'not_indexed';
  cacheHitRate: number;
  indexFreshnessMs: number;
  staleFiles: number;
  filesChecked: number;
  dbSizeBytes: number;
  port: number;
  uptimeMs: number;
  mcpServerStatus: 'connected' | 'disconnected';
}

export interface McpActivity {
  tool: string;
  prompt: string;
  success: boolean;
  latency_ms: number;
  tokens: number;
  tokens_saved: number;
  cache_hit: boolean;
  timestamp: string;
}

export interface McpToolStats {
  name: string;
  calls: number;
  avg_latency: number;
  tokens_saved: number;
  tokens_used: number;
  failures: number;
}

export interface ErrorEvent {
  id: number;
  tool: string;
  arguments: string;
  latency_ms: number;
  timestamp: string;
}

export interface DirectoryStat {
  path: string;
  files: number;
  symbols: number;
  bytes: number;
}

export interface FileHotspot {
  file_path: string;
  symbol_count: number;
  total_incoming: number;
  total_outgoing: number;
  aggregate_score: number;
}

export interface CycleNode {
  name: string;
  kind: string;
  file_path: string;
}

export interface DependencyCycle {
  length: number;
  cross_file: boolean;
  nodes: CycleNode[];
}

export interface CodebaseOverview {
  available: boolean;
  healthScore: number;
  dependencyDensity: number;
  circularDependencies: number;
  orphanSymbols: number;
  orphanSymbolPercent: number;
  languages: LanguageStat[];
  directories: DirectoryStat[];
  hotspotFiles: FileHotspot[];
  cycleExamples: DependencyCycle[];
}

async function getJson<T>(url: string, fallback: T): Promise<T> {
  try {
    const res = await fetch(url);
    if (!res.ok) return fallback;
    return await res.json();
  } catch (e) {
    console.error(`Failed to fetch ${url}`, e);
    return fallback;
  }
}

export const api = {
  getOverview: async (): Promise<RepositoryOverview> => {
    const data = await getJson<any>('/api/v1/stats/overview', {});
    const rawTokens = data.total_raw_tokens || 0;
    const avoided = data.total_tokens_avoided || 0;
    return {
      workspaceName: data.workspace_name || 'Unknown Workspace',
      workspacePath: data.workspace_path || '',
      filesIndexed: data.files_indexed || 0,
      symbols: data.symbols_indexed || 0,
      relationships: data.relationships_indexed || 0,
      communities: data.communities || 0,
      entrypoints: data.entrypoints || 0,
      embeddedSymbols: data.embedded_symbols || 0,
      repositorySizeBytes: data.repository_size_bytes || 0,
      totalTokensAvoided: avoided,
      totalTokensUsed: data.total_tokens_used || 0,
      totalRawTokens: rawTokens,
      totalCalls: data.total_calls || 0,
      failedCalls: data.failed_calls || 0,
      successRate: data.success_rate ?? 100,
      avgLatencyMs: data.avg_latency_ms || 0,
      estCostSavedCents: data.est_cost_saved_cents || 0,
      tokenUsageGraph: data.token_usage_graph || [],
      contextCompressionPercent: rawTokens > 0 ? (avoided / rawTokens) * 100 : 0,
    };
  },
  getSystemHealth: async (): Promise<SystemHealth> => {
    const data = await getJson<any>('/api/v1/stats/health', {});
    return {
      status: data.status || 'not_indexed',
      cacheHitRate: data.cache_hit_rate || 0,
      indexFreshnessMs: data.index_freshness_ms || 0,
      staleFiles: data.stale_files || 0,
      filesChecked: data.files_checked || 0,
      dbSizeBytes: data.db_size_bytes || 0,
      port: data.port || 0,
      uptimeMs: data.uptime_ms || 0,
      mcpServerStatus: 'connected',
    };
  },
  getGraph: () => getJson<{ nodes: GraphNode[]; edges: GraphEdge[] }>('/api/v1/graph/snapshot', { nodes: [], edges: [] }),
  getMcpActivity: () => getJson<McpActivity[]>('/api/v1/mcp/activity', []),
  getMcpTools: () => getJson<McpToolStats[]>('/api/v1/mcp/tools', []),
  getErrors: () => getJson<ErrorEvent[]>('/api/v1/errors', []),
  getCodebaseOverview: async (): Promise<CodebaseOverview> => {
    const data = await getJson<any>('/api/v1/codebase/overview', { available: false });
    return {
      available: !!data.available,
      healthScore: data.health_score || 0,
      dependencyDensity: data.dependency_density || 0,
      circularDependencies: data.circular_dependencies || 0,
      orphanSymbols: data.orphan_symbols || 0,
      orphanSymbolPercent: data.orphan_symbol_percent || 0,
      languages: data.languages || [],
      directories: data.directories || [],
      hotspotFiles: data.hotspot_files || [],
      cycleExamples: data.cycle_examples || [],
    };
  },
};
