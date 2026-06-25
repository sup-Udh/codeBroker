# Phase 6: Graph Correctness & Symbol-Level Dependency Integrity

## Executive Summary
In Phase 6, we completely overhauled CodeBroker's graph query engine to eliminate false dependencies and relationship leakage. Previous versions of CodeBroker suffered from "file-level aggregation," where dependencies were tracked using `source_file_id`, causing unrelated symbols in the same file to inherit each other's connections. 

By centralizing all queries through a new Universal Graph API (`query/src/graph.rs`) using strict `symbol_id` lookups, we guarantee that every relationship returned by CodeBroker accurately reflects the true symbol-level call graph.

## Key Improvements

### 1. Centralized Graph API
- Added `get_incoming_edges` and `get_outgoing_edges` to `query/src/graph.rs`.
- All higher-level Context Builder methods (e.g., `fetch_callers`, `fetch_callees`, `fetch_forward_dependencies`) now funnel through this single source of truth.
- Completely removed `source_file_id = ?1` logic from the Context Builder, preventing dependency leakage across sibling symbols in the same file.

### 2. Entrypoint Scoring System
- Replaced the binary heuristic for detecting entrypoints (e.g., hardcoded kinds like `route`) with a flexible `calculate_entrypoint_score` system.
- The scoring system dynamically evaluates:
  - **Syntax / Kinds**: Routes, endpoints, pages, and layouts.
  - **Parser Attributes**: Decorators containing generic signals (`get`, `post`, `route`, `endpoint`).
  - **Graph Structure**: High rewards for zero incoming static call edges; penalties for many incoming static calls.
  - **Naming Patterns**: Small heuristic bumps for names containing `handler` or `main`.
- This ensures CodeBroker remains entirely language- and framework-agnostic while reducing false positives.

### 3. Eliminated Self-Loops and Duplicate Edges
- Graph traversal explicitly prevents self-loops (`source_symbol_id != target_symbol_id`).
- Same-file callers are now carefully checked against incoming edges, rather than via string matching on the file body, completely removing regex/string-based false positives.

### 4. Graph Invariant Automation
- Added a robust test suite (`mcp/tests/graph_validation_tests.rs`) that randomly samples symbols from the production database to verify three critical invariants:
  1. No orphaned edges (all edges point to valid symbols).
  2. No duplicate edges.
  3. Absolute parity between Context Assembly (`get_context`) and Graph Traversal (`explore_graph` depth=1).

## Impact on Benchmarks
These changes resolve the core "Graph Correctness" issues identified in previous evaluations. `get_context` now reliably provides symbol-level accuracy without overwhelming the LLM with false positive dependencies from unrelated components sharing the same source file.
