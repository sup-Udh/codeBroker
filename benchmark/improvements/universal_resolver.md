# Phase 11 — Universal Resolver & MCP Architecture Rebuild

## Overview

Phase 11 introduced a **Universal Resolver** (`resolver/` crate) — a deterministic, honest
resolution pipeline that all MCP tools delegate to. No tool may implement its own ambiguity
detection, confidence scoring, or fuzzy matching. The resolver is the single source of truth
for turning arbitrary user input into a canonical entity.

---

## Architecture Overview

```
User input (symbol name, file path, subsystem name, concept)
         │
         ▼
  resolver::resolve_any()          ← full pipeline
  ├─ resolve_symbol()              ← symbol-specific path
  │   ├─ stage_exact_symbol()      SQL: symbols.name = ?  (case-sensitive)
  │   ├─ stage_canonical_symbol()  SQL: LOWER(name) = LOWER(?)
  │   └─ stage_semantic_symbol()   cosine similarity + ambiguity margin
  ├─ resolve_path()                ← file/directory path
  │   ├─ stage_full_path()         SQL: files.path = ?
  │   ├─ stage_filename()          SQL: path LIKE '%name'
  │   └─ stage_directory()        filesystem + indexed children
  ├─ resolve_subsystem()          ← subsystem name
  │   └─ discover_subsystem()     seed + graph expansion (unchanged algorithm)
  └─ stage_feature()              ← concept/domain name (exact match only)
         │
         ▼
  ResolvedEntity  (Symbol | File | Directory | Subsystem | Feature | Ambiguous | NotFound)
         │
         ▼
  MCP tool (thin wrapper — reacts to variant, never resolves itself)
```

### Invariants

- **Deterministic**: same DB state + same input → same output, always.
- **Honest**: ambiguous queries return `Ambiguous`; unknown queries return `NotFound` with `stages_tried`. No silent first-match guesses.
- **Single confidence model**: `Confidence { score: u8, label: High/Medium/Low, reasons: Vec<String> }` computed once in resolver, never recomputed per tool.
- **No hardcoded fallbacks**: removed all `LIMIT 1 WITHOUT ORDER`, all word-split fuzzy heuristics, all case-insensitive partial matches outside canonical stage.

---

## Resolver Design

### `ResolvedEntity` variants

| Variant | When returned |
|---------|---------------|
| `Symbol(ResolvedSymbol)` | Exactly one symbol matched with sufficient confidence |
| `File(ResolvedFile)` | Path uniquely identified a file |
| `Directory(ResolvedDirectory)` | Path identified a directory with indexed children |
| `Subsystem(ResolvedSubsystem)` | Named subsystem discovered with High or Medium confidence |
| `Feature(ResolvedFeature)` | Exact match against an indexed concept name |
| `Ambiguous(AmbiguousMatch)` | Two or more candidates are too close to pick; returns candidate list + hint |
| `NotFound(NotFound)` | Nothing matched at any stage; returns `stages_tried` list |

### `Confidence` model

```
score 80–100 → High
score 50–79  → Medium
score  0–49  → Low
```

- Exact symbol: 95
- Canonical (case-insensitive) unique: 80
- Semantic unique (≥ 0.35 cosine, margin > 0.03): 35 + scaled by cosine
- Full path: 95
- Filename suffix unique: 75
- Subsystem High/Medium: 90/65 (Low → NotFound, never returned)
- Feature exact: 85

### Ambiguity detection

Two candidates are "too close" if either:
- Both returned from DB with equal match quality (exact/canonical stage), or
- Semantic scores are within `SEMANTIC_AMBIGUITY_MARGIN = 0.03` of each other.

In these cases the resolver returns `Ambiguous` with all candidates listed. The MCP tool
serializes this as a JSON error with `"candidates"` array, guiding the user to add a
`file_path` hint to disambiguate.

---

## Files Created

| File | Purpose |
|------|---------|
| `resolver/Cargo.toml` | New crate; depends on `storage`, `query`, `graph` |
| `resolver/src/lib.rs` | Public API + documented limitations |
| `resolver/src/types.rs` | All resolver types: `ResolvedEntity`, `Confidence`, `Candidate`, `NotFound`, `AmbiguousMatch`, `ResolvedSymbol`, `ResolvedFile`, `ResolvedDirectory`, `ResolvedSubsystem`, `ResolvedFeature`, `EntityType`, `ConfidenceLabel` |
| `resolver/src/pipeline.rs` | All pipeline stages + 11 unit tests |
| `storage/src/entrypoints.rs` | `classify_entrypoint()` — framework-agnostic FastAPI + Next.js App Router entrypoint detection with 7 unit tests |

---

## Files Modified

| File | Change |
|------|--------|
| `Cargo.toml` | Added `"resolver"` to workspace members |
| `mcp/Cargo.toml` | Added `resolver` dependency |
| `mcp/src/main.rs` | All symbol-keyed tools now call `resolve_symbol_for_tool()`; path tools call `resolve_path()`; subsystem tools gate on `resolve_subsystem()` |
| `storage/src/lib.rs` | Added `pub mod entrypoints;` |
| `indexer/src/features.rs` | `is_entrypoint` via `classify_entrypoint_json()` instead of ad-hoc decorator check |
| `indexer/src/reindex.rs` | Whole-name case-sensitive linker pass (removed word-split + case-insensitive fallback) |
| `parser/src/python_frontend.rs` | Type annotation extraction (`typed_parameter`, `typed_default_parameter`, return type) with `kind: Some("type_ref")` |
| `parser/src/normalize.rs` | Returns `Option<(String, usize)>` node count; boilerplate filter for logger/get_logger |
| `query/src/duplicates.rs` | Uses node count from normalize for threshold; `Option<(String, usize)>` signature |
| `query/src/context.rs` | `fetch_reverse_dependencies`/`fetch_forward_dependencies` use `CANONICAL_DEPENDENCY_EDGES` slice |
| `query/src/graph.rs` | `CANONICAL_DEPENDENCY_EDGES` constant; `explore_graph_scoped` with `file_hint`; `graph_subtree` with `file_hint` + `truncated_reason`; `get_incoming/outgoing_edges` take `Option<&[&str]>` |
| `query/src/subsystem.rs` | `list_entrypoints` + `discover_subsystem` use `classify_entrypoint_json`; `subsystem_communication` rebuilt on file-id edge scanning |
| `query/src/retrieval.rs` | `read_file_snippet` validates line ranges; `skeletonize_file` handles directory paths with indexed children list |
| `cli/src/main.rs` | Init path calls `infer_interactions` + `extract_features` after linking; full linker fallback uses whole-name case-sensitive resolution |
| `semantic/src/openai.rs` | `.http1_only()` on reqwest client (HTTP/2 POST hang on transparent proxy) |

---

## Removed Hardcoded Fallbacks

| Location | What was removed | Why it was wrong |
|----------|-----------------|-----------------|
| `mcp/src/main.rs` `check_symbol_ambiguity` | `LIMIT 1` first-match on ambiguous symbols | Silently picked wrong symbol |
| `indexer/src/reindex.rs` linker Pass 2 | Word-split + `LOWER(name) LIKE LOWER(?)` | `topology_agent` → `topology` matched `Topology` class (phantom edge) |
| `query/src/context.rs` | Hardcoded `Some("imports")` for dependency edges | Missed `calls`, `type_ref`, `interaction`, `component_use` edges |
| `query/src/graph.rs` `architectural_hotspots` | Hardcoded duplicate `"calls"` in edge list | Inflated hotspot scores |
| `indexer/src/features.rs` | Per-symbol decorator string check in feature loop | Replaced by `classify_entrypoint_json` which is tested and framework-aware |

---

## New MCP Tool Contracts

All symbol-keyed tools now respond identically when resolution fails:

```json
{
  "error": "ambiguous",
  "query": "Router",
  "candidates": [
    {"name": "Router", "kind": "class", "file_path": "api/router.py", "start_line": 12},
    {"name": "Router", "kind": "class", "file_path": "web/router.ts", "start_line": 3}
  ],
  "hint": "Pass file_path to disambiguate"
}
```

Or for NotFound:

```json
{
  "error": "not_found",
  "query": "zzz_ghost",
  "reason": "No symbol matching 'zzz_ghost' found at any resolution stage.",
  "stages_tried": ["exact", "canonical", "semantic"]
}
```

Tools that added a `file_path` parameter as part of this phase:
- `explore_graph` (new `file_path` schema field)
- `graph_subtree` (existing `file_path` field now passed through to resolver)
- `read_symbol_source` (multi-target; each target resolved independently)

---

## Before vs. After Architecture

### Before

```
MCP tool
  └─ check_symbol_ambiguity()   ← returns error string OR (name, file_hint) pair
       └─ raw SQL LIMIT 1        ← guesses silently if >1 result
  └─ query::*()                  ← each tool picked its own edge filter string
  └─ confidence                  ← each tool computed ad-hoc (or didn't)
```

### After

```
MCP tool (thin wrapper)
  └─ resolve_symbol_for_tool()   ← single call, returns ResolvedSymbol or error JSON
       └─ resolver::resolve_symbol()
            ├─ stage_exact         ← deterministic
            ├─ stage_canonical     ← deterministic  
            └─ stage_semantic      ← with ambiguity margin
  └─ query::*()                   ← called with already-resolved (name, file_hint)
  └─ confidence                   ← one Confidence struct from resolver, propagated
```

---

## Validation Results

All 58 workspace tests pass after Phase 11 (cargo test --workspace):

| Crate | Tests | Result |
|-------|-------|--------|
| indexer (call_edge) | 5 | ✅ |
| indexer (reindex) | 2 | ✅ |
| mcp | 7 | ✅ |
| graph invariants | 1 | ✅ |
| parser | 5 | ✅ |
| query | 17 | ✅ |
| resolver (pipeline) | 11 | ✅ |
| semantic | 3 | ✅ |
| storage (entrypoints) | 7 | ✅ |

### Smoke tests run against `/home/labuser/Downloads/netwin` index

| Test | Before | After |
|------|--------|-------|
| `graph_subtree("Router")` — two files named Router | Silent first-match | Returns `Ambiguous` with both candidates |
| `subsystem_stats("zzz_nonexistent_xyz")` | Medium-confidence bogus result | `NotFound` |
| `read_file_snippet` out-of-range lines | Empty string | Error with file line count |
| `read_file_skeleton` on directory path | Generic "not found" | Lists indexed children |
| Python `simulate(topology: Topology)` → edge to `Topology` class | Missing (no type_ref) | Present (`kind=type_ref`) |
| FastAPI `@app.get("/items")` → `is_entrypoint=true` | 0 entrypoints on fresh init | Correct after `extract_features` added to init |
| `subsystem_communication` A↔B edge count | Contradicted `subsystem_stats` | Consistent (file-id-based scan) |

---

## Remaining Limitations

1. **Alias stage not implemented**: the pipeline has no alias table. If a symbol is known by
   multiple names (e.g. re-exported under a different name in an index file), the resolver
   will not find it via the alias. A future "Alias Match" stage would look up `symbol_aliases`
   (not yet created) before falling through to semantic search.

2. **`generate_context_capsule` not migrated**: this tool is a multi-pivot aggregator that calls
   many sub-queries internally. It was left unchanged in Phase 11 because migrating it would
   require threading `ResolvedEntity` through its entire internal pipeline. It retains its own
   heuristic confidence scoring. A future phase should refactor it to use `resolve_any` per pivot.

3. **`subsystem_communication` double-resolves**: `subsystem_stats`/`subsystem_communication`
   both call `resolve_subsystem()` as a gate, then call `discover_subsystem()` again internally
   for the actual data. The resolver's first call is wasted work. A future refactor could pass
   the resolved `SubsystemStats` through rather than re-querying.

4. **Semantic stage requires embeddings**: if no embeddings have been generated (e.g., no API key
   configured), the semantic stage silently falls through to NotFound rather than returning a
   degraded-but-useful canonical result. Users with no embedding provider see worse symbol
   resolution than users with one configured.
