# Phase 10 — Correctness & Consistency

## Overview
Phase 10 focuses on ensuring the underlying data model is internally consistent, eliminating discrepancies where different MCP tools report conflicting findings based on the same indexed codebase.

## Improvements

### 1. Entrypoint Detection
- **Previous Heuristic:** Entrypoints were naively inferred when a symbol had zero internal callers (`fan_in == 0`) and contained an `@` decorator in its attributes. This frequently failed in frameworks like Flask, FastAPI, or Next.js App Router, where entrypoints (like route handlers) might be internally imported elsewhere for testing or typing, causing `fan_in > 0` and breaking the entrypoint heuristic.
- **Fix:** Entrypoints are now strictly an indexer-owned parser fact. The parser marks symbols as entrypoints if they are callable and contain relevant router decorators (e.g. `@app.route`, `app.get`, `app.post`) without relying on the fragile `fan_in == 0` heuristic.

### 2. Graph Consistency
- **Previous Issue:** Some tools, like `architectural_hotspots`, filtered edges by simply counting `COUNT(*)` over all edge types, including noise like `registration` or undocumented pseudo-edges. Other tools (e.g., `get_edit_context`) only looked at `imports`. This led to highly divergent dependency counts across MCP tools.
- **Fix:** Defined a `CANONICAL_DEPENDENCY_EDGES` constant (`calls`, `imports`, `interaction`, `component_use`) in `query/src/graph.rs`. `architectural_hotspots`, `fetch_forward_dependencies`, and `fetch_reverse_dependencies` now universally filter against this canonical set, guaranteeing consistent graph counts and preventing noisy tool discrepancies.

### 3. Duplicate Logic Deduplication
- **Previous Issue:** `find_duplicate_logic` relied on a naive `min_normalized_len` (string length minimum) when comparing AST fingerprints. This mistakenly grouped trivial single-line assignments, imports, and logger initializations (`logger = get_logger(__name__)`) as "duplicates" simply because their serialized AST strings surpassed the string-length threshold.
- **Fix:** Transitioned from a string-length minimum to a structural minimum. `parser::normalize::normalize_snippet` now returns the Tree-sitter AST subtree fingerprint alongside its structural node count. Trivial boilerplate is explicitly excluded (e.g., logger setup), and logic chunks require a baseline structural depth/node-count (15+ AST nodes) to qualify as duplicates, dramatically reducing false positives and improving detection precision.
