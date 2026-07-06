import React, { useCallback, useEffect, useState } from 'react';
import ReactFlow, {
  MiniMap,
  Controls,
  Background,
  useNodesState,
  useEdgesState,
  addEdge,
  Connection,
  Edge,
  Node,
  MarkerType
} from 'reactflow';
import 'reactflow/dist/style.css';
import { api, GraphNode, GraphEdge } from '../../providers/api';
import { useLiveEvents } from '../../hooks/useLiveEvents';

const nodeColor = (node: Node) => {
  switch (node.type) {
    case 'directory': return '#71717a';
    case 'file': return '#ff6b35';
    case 'function': return '#22c55e';
    default: return '#fafafa';
  }
};

export function SemanticGraph() {
  const [nodes, setNodes, onNodesChange] = useNodesState<any>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<any>([]);
  const [loading, setLoading] = useState(true);

  const loadData = useCallback(async () => {
    try {
      const data = await api.getGraph();
      // Very basic layout generation for dummy data
      const reactFlowNodes: Node[] = data.nodes.map((n: GraphNode, i: number) => ({
        id: n.id,
        type: 'default',
        data: { label: n.label },
        position: { x: (i % 3) * 200, y: Math.floor(i / 3) * 150 },
        style: {
          background: '#111113',
          color: '#fafafa',
          border: '1px solid #1f1f22',
          borderRadius: '8px',
          width: 150,
          boxShadow: n.type === 'file' ? '0 0 10px rgba(255,107,53,0.2)' : 'none'
        }
      }));

      const reactFlowEdges: Edge[] = data.edges.map((e: GraphEdge) => ({
        id: e.id,

        source: e.source,
        target: e.target,
        animated: true,
        style: { stroke: '#71717a' },
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: '#71717a',
        },
      }));

      setNodes(reactFlowNodes);
      setEdges(reactFlowEdges);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadData();
  }, [loadData]);

  useLiveEvents((type) => {
    if (type === 'index_update') {
      loadData();
    }
  });

  const onConnect = useCallback((params: Edge | Connection) => setEdges((eds) => addEdge(params, eds)), [setEdges]);

  if (loading) {
    return <div className="text-[#71717a] flex items-center justify-center h-full">Loading graph...</div>;
  }

  return (
    <div className="space-y-4 h-[calc(100vh-10rem)] flex flex-col animate-in fade-in duration-500">
      <div className="flex justify-between items-center flex-shrink-0">
        <h2 className="text-xl font-semibold">Semantic Graph</h2>
        <div className="flex gap-4 text-xs">
          <div className="flex items-center gap-2"><span className="w-2 h-2 rounded-full bg-[#71717a]"></span> Directories</div>
          <div className="flex items-center gap-2"><span className="w-2 h-2 rounded-full bg-[#ff6b35]"></span> Files</div>
          <div className="flex items-center gap-2"><span className="w-2 h-2 rounded-full bg-[#22c55e]"></span> Functions</div>
        </div>
      </div>
      <div className="bg-[#111113] border border-[#1f1f22] rounded-xl flex-1 overflow-hidden relative">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          fitView
          className="bg-[#09090b]"
        >
          <Controls className="bg-[#111113] border-[#1f1f22] fill-[#fafafa]" />
          <MiniMap nodeColor={nodeColor} nodeStrokeWidth={3} zoomable pannable style={{ backgroundColor: '#111113', border: '1px solid #1f1f22' }} />
          <Background color="#1f1f22" gap={24} size={1} />
        </ReactFlow>
      </div>
    </div>
  );
}
