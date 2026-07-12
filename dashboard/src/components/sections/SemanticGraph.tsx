import React, { useCallback, useEffect, useMemo, useState } from 'react';
import ReactFlow, {
  MiniMap,
  Controls,
  Background,
  useNodesState,
  useEdgesState,
  Handle,
  Position,
  Edge,
  Node,
  MarkerType,
  NodeProps,
} from 'reactflow';
import 'reactflow/dist/style.css';
import { api, GraphNode, GraphEdge } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';
import { LoadingState, EmptyState, SectionHeading } from '../ui';
import { Network } from 'lucide-react';

function GraphNodeCard({ data }: NodeProps) {
  return (
    <div
      className="rounded-lg border px-3 py-2 bg-[#111113] min-w-[150px] max-w-[190px]"
      style={{ borderColor: data.color, boxShadow: `0 0 0 1px ${data.color}22` }}
    >
      <Handle type="target" position={Position.Left} style={{ background: data.color, width: 6, height: 6 }} />
      <Handle type="source" position={Position.Right} style={{ background: data.color, width: 6, height: 6 }} />
      <div className="flex items-center gap-1.5 mb-1">
        <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ backgroundColor: data.color }} />
        <span className="text-[10px] uppercase tracking-wide text-[#71717a] truncate">{data.kind}</span>
      </div>
      <div className="text-xs font-mono text-[#fafafa] truncate" title={data.label}>{data.label}</div>
      <div className="text-[10px] text-[#71717a] truncate mt-0.5" title={data.filePath}>{data.filePath}</div>
      <div className="text-[10px] text-[#71717a] mt-1">{data.connections} connections</div>
    </div>
  );
}

const nodeTypes = { codebroker: GraphNodeCard };

export function SemanticGraph() {
  const [nodes, setNodes, onNodesChange] = useNodesState<any>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<any>([]);
  const [loading, setLoading] = useState(true);
  const [empty, setEmpty] = useState(false);

  const loadData = useCallback(async () => {
    try {
      const data = await api.getGraph();
      if (data.nodes.length === 0) {
        setEmpty(true);
        setLoading(false);
        return;
      }
      setEmpty(false);

      const cols = Math.max(1, Math.ceil(Math.sqrt(data.nodes.length * 1.6)));
      const reactFlowNodes: Node[] = data.nodes.map((n: GraphNode, i: number) => ({
        id: n.id,
        type: 'codebroker',
        data: { label: n.label, kind: n.kind, filePath: n.file_path, connections: n.connections, color: n.color },
        position: { x: (i % cols) * 210, y: Math.floor(i / cols) * 110 },
      }));

      const reactFlowEdges: Edge[] = data.edges.map((e: GraphEdge, i: number) => ({
        id: `e-${i}-${e.source}-${e.target}`,
        source: e.source,
        target: e.target,
        style: { stroke: '#3f3f46', strokeWidth: 1 },
        markerEnd: { type: MarkerType.ArrowClosed, color: '#3f3f46', width: 14, height: 14 },
      }));

      setNodes(reactFlowNodes);
      setEdges(reactFlowEdges);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, [setNodes, setEdges]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  useLiveEvents((type) => {
    if (type === 'index_update') {
      loadData();
    }
  });

  const communityLegend = useMemo(() => {
    const seen = new Map<number, string>();
    for (const n of nodes) {
      if (!seen.has(n.data.community) && n.data.color) seen.set(n.data.community ?? 0, n.data.color);
    }
    return Array.from(seen.entries()).slice(0, 6);
  }, [nodes]);

  if (loading) {
    return <LoadingState label="Loading dependency graph…" />;
  }

  return (
    <div className="space-y-4 h-[calc(100vh-10rem)] flex flex-col animate-in fade-in duration-500">
      <SectionHeading
        title="Semantic Graph"
        subtitle="Top symbols ranked by real PageRank + connection count, clustered by real community detection"
        action={
          communityLegend.length > 0 ? (
            <div className="flex gap-3 text-xs text-[#71717a] flex-wrap justify-end max-w-xs">
              {communityLegend.map(([id, color]) => (
                <div key={id} className="flex items-center gap-1.5">
                  <span className="w-2 h-2 rounded-full" style={{ backgroundColor: color }} />
                  Cluster {id}
                </div>
              ))}
            </div>
          ) : undefined
        }
      />
      <div className="bg-[#111113] border border-[#1f1f22] rounded-xl flex-1 overflow-hidden relative">
        {empty ? (
          <div className="h-full flex items-center justify-center">
            <EmptyState icon={<Network className="w-8 h-8" />} title="No graph data yet" subtitle="Index a workspace with codebroker index to populate the dependency graph." />
          </div>
        ) : (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            fitView
            className="bg-[#09090b]"
          >
            <Controls className="!bg-[#111113] !border-[#1f1f22] !fill-[#fafafa] [&>button]:!border-[#1f1f22] [&>button]:!bg-[#111113] [&>button:hover]:!bg-[#1f1f22]" />
            <MiniMap nodeColor={(n: any) => n.data?.color || '#71717a'} nodeStrokeWidth={2} zoomable pannable style={{ backgroundColor: '#111113', border: '1px solid #1f1f22' }} />
            <Background color="#1f1f22" gap={28} size={1} />
          </ReactFlow>
        )}
      </div>
    </div>
  );
}
