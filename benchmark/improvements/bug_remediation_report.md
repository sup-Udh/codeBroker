# CodeBroker Bug Remediation Report

**Source spec:** `benchmark/bug_report.md`
**Test repo:** `/home/labuser/code/netwin` (OrcaAI — FastAPI orchestrator + Next.js/React frontend).
**Validation method:** every fix was reproduced live before the change, then re-verified after a full clean reindex (`codebroker init`) by driving the installed `codebroker-mcp` binary over JSON-RPC and by inspecting the SQLite index directly.

The guiding principle of this pass was the one stated in the brief: **fix shared infrastructure, not individual MCP tools.** Where a bug surfaced in one tool, the fix was pushed down into the parser, the linker, feature extraction, or a new shared classifier so that every tool benefits automatically and the class of bug cannot reappear per-tool.

---

## Summary table

| Bug | Title | Status | Layer the fix actually lives in |
|----|-------|--------|----------------------------------|
| 1 | Entrypoint/route detection returns zero | **Fixed** | New shared classifier (`storage::entrypoints`) + feature extraction + full-index pipeline |
| 2 | Reverse-dep edges keyed on param name, not type | **Fixed** | Parser (type-annotation extraction) + graph linker (whole-name resolution) |
| 3 | `subsystem_communication` vs `subsystem_stats` contradiction | **Fixed** | Unified edge basis in `query::subsystem` |
| 4 | `generate_context_capsule` confidently wrong | **Fixed** | Capsule pivot selection + confidence derivation |
| 5 | `graph_subtree` silent ambiguity | **Fixed** | Shared ambiguity guard + file scoping + truncation reason |
| 6 | `read_file_snippet` out-of-range returns empty | **Fixed** | `query::retrieval` range validation |
| 7 | `read_file_skeleton` on a directory gives generic error | **Fixed** | Index-based directory detection in `query::retrieval` |

A significant **additional bug** was discovered and fixed while working on Bug 1 — see the *Additional issues discovered* section.

---

## Bug 1 — Entrypoint/route detection returns zero

**Status: Fixed.**

### Root cause
Two independent problems, plus a third discovered during the fix:

1. **Detection was a fragile, kind-specific heuristic.** `indexer::features::extract_features` marked a symbol as an entrypoint only if `kind ∈ {route, endpoint, page, layout}` (kinds the parsers never actually emit for Python/Next.js) or, in a later patch, if its attributes `contains("@")`. The `contains("@")` form both **over-matched** (`@staticmethod`, `@property`, `@dataclass` became "entrypoints") and **under-described** the real routes.
2. **There was no Next.js App Router support at all.** Nothing recognised the `app/**/page.tsx` / `layout.tsx` file convention, so those entrypoints reported zero regardless of detection logic.
3. **The full-index path never computed features at all** (see *Additional issues discovered #A*). Even with perfect detection logic, a freshly `codebroker init`-ed repo had an empty `symbol_features` table, so `is_entrypoint` was never written.

### Architectural fix
- Added a **single shared classifier**, `storage::entrypoints::classify_entrypoint(name, kind, path, attributes) -> Option<EntrypointClass::{Route,Page}>`. It understands:
  - decorator routes via `is_route_decorator` (`@app.get`, `@router.post`, `@app.websocket`, `@app.route`, `@bp.route`, bare `@get` …) while explicitly rejecting `@staticmethod`/`@property`/`@dataclass`/`@pytest.fixture`;
  - explicit `route`/`endpoint`/`page`/`layout` kinds;
  - Next.js **App Router** (`app/**/page|layout|template`, `app/**/route.ts`) and **Pages Router** (`pages/**`, `pages/api/**`), gated on the React component-casing convention so co-located helpers aren't flagged.
- `extract_features` now joins `files.path` and calls the classifier to set `is_entrypoint`.
- `list_entrypoints` and `discover_subsystem` now derive the **route-vs-page split from the same classifier**, so detection and categorisation can never disagree.

### Files modified
- `storage/src/entrypoints.rs` (new), `storage/src/lib.rs`
- `indexer/src/features.rs`
- `query/src/subsystem.rs`
- `cli/src/main.rs` (full-index now runs feature extraction — see Additional #A)

### Before vs after
```
list_entrypoints()  BEFORE: { total: 0, routes: [], pages: [] }
list_entrypoints()  AFTER:  { total: 19, routes: 17, pages: 2 }
  pages = [ RootLayout @ OrcaAI/frontend/app/layout.tsx,
            Home       @ OrcaAI/frontend/app/page.tsx ]
  routes include all 8 FastAPI routes in main.py (incl. both @app.websocket),
  the @router.post import_topology route, and the Flask container app.py routes.
```

### Remaining limitations
Next.js page detection marks component-cased callables in convention files; a deliberately lowercase default-exported page component would be missed (rare, and the safer failure direction).

---

## Bug 2 — Reverse-dependency edges keyed on parameter name, not resolved type

**Status: Fixed.**

### Root cause (deeper than the report hypothesised)
The report guessed "matching on the parameter identifier name." The real cause was a pair of compounding defects in the **graph linker**, confirmed by inspecting the edges into `Topology`:

- **Phantom edges from word-splitting + case-insensitive matching.** The fallback linker split every import/reference name on non-alphanumeric boundaries and matched each fragment with `LOWER(name) = LOWER(?)`. So `from ai.topology_understanding_agent import topology_agent` split into `topology` + `agent`, and the `topology` fragment matched the `Topology` class — making `analyze_topology` a phantom dependent. The same mechanism made `compile_topology` (used inside `simulate`) fabricate a second phantom `simulate --instantiates--> Topology`.
- **The genuine dependency was never captured.** `def simulate(topology: Topology)` expresses a real dependency through the *type annotation*, but the Python parser extracted no edge for parameter/return type annotations at all.

Net effect: the one true dependent (`simulate`) was invisible while a non-dependent (`analyze_topology`) was reported.

### Architectural fix
- **Parser:** `python_frontend` now extracts parameter and return **type annotations** as `type_ref` edges (`typed_parameter`, `typed_default_parameter`, `return_type`, including the inner identifier of `Subscript` generics like `List[Topology]`).
- **Linker (both the full `cli` linker and incremental `reindex_paths`):** the word-split + case-insensitive fallback was replaced with **whole-name, case-sensitive** resolution (local-first, then global exact). A compound identifier is one name; sub-word cross-case matching is a coincidence, not a reference. `GENERIC_SYMBOL_NAMES` filtering is preserved.
- `type_ref` was added to `CANONICAL_DEPENDENCY_EDGES` so impact-analysis/dependency listing count it uniformly.

### Files modified
- `parser/src/python_frontend.rs`
- `cli/src/main.rs`, `indexer/src/reindex.rs`
- `query/src/graph.rs` (`CANONICAL_DEPENDENCY_EDGES`)

### Before vs after
```
impact_analysis("Topology").reverse_dependencies
  BEFORE: ["analyze_topology"]      (wrong: that fn takes `topology: dict`)
  AFTER:  ["simulate"]              (correct: `def simulate(topology: Topology)`)

DB edges INTO Topology
  BEFORE: imports(analyze_topology), instantiates(simulate-from-compile_topology), + more phantoms
  AFTER:  type_ref(simulate)        (only the real one)
```

### Tests added
- `parser::python_frontend::type_ref_tests` (annotation → `type_ref`; param identifier never treated as a type).
- `indexer::reindex::tests::type_annotation_drives_dependency_not_param_name_or_subword` (end-to-end: `simulate` is a dependent of `Topology`, `analyze` is not).

---

## Bug 3 — `subsystem_communication` contradicts `subsystem_stats`

**Status: Fixed.**

### Root cause
The two tools read **different edge bases**. `subsystem_stats` computes `consumers`/`dependencies` at **file granularity** (`edges.source_file_id`), which includes edges whose `source_symbol_id` is NULL (top-level imports/`fetch`es with no enclosing symbol). `subsystem_communication` only counted edges where `source_symbol_id IS NOT NULL` and both ends resolved to a symbol — so the very frontend→orchestrator import edges that `consumers` listed were invisible to it, yielding `0/0`.

### Architectural fix
`subsystem_communication` now keys on **file membership** (`source_file_id` and the target symbol's `file_id`) — the identical basis `discover_subsystem` uses for `consumers`/`dependencies`. Edges with a NULL enclosing symbol are included, labelled by the source file's basename. The two tools can no longer disagree about whether an A↔B edge exists.

### Files modified
- `query/src/subsystem.rs`

### Before vs after
```
subsystem_communication("orchestrator","frontend")
  BEFORE: { a_to_b_edges: 0, b_to_a_edges: 0, examples: [] }
  AFTER:  { a_to_b_edges: 0, b_to_a_edges: 2,
            b_to_a_examples: [["NodeConfigPanel.tsx","Node"],
                              ["TopologyCanvas.tsx","Edge"]] }
```
This now agrees with `subsystem_stats("orchestrator").consumers`, which lists those same frontend components.

---

## Bug 4 — `generate_context_capsule` confidently wrong

**Status: Fixed.**

### Root cause
Two distinct defects:
1. **Bare data declarations could become pivots.** A lexical token collision (the query word "metrics" matching the empty `METRICS_QUEUES = {}` dict, or byte-math locals like `rx_bytes`) let non-callable symbols win pivot slots, even though a capsule renders each pivot's *full implementation*.
2. **The confidence label was decoupled from answer quality.** `"High (Subsystem Validated)"` was applied whenever the subsystem graph was connected, regardless of whether the chosen pivots were relevant — exactly what made the label "confidently wrong." (A latent bug also made the base confidence a lexicographic `max()` over strings, ranking `"Medium…"` above `"High…"`.)

### Architectural fix
- **Pivot selection:** demote `variable`/`local`/`parameter`/`field`/`property` kinds so a callable/route/class/component always wins a pivot slot when available. This is a property of *what a capsule pivot is*, not a per-query heuristic.
- **Confidence:** take the top (highest-scored) pivot's confidence, and only promote to `"High (Subsystem Validated)"` when that pivot is **itself** high-confidence. Structural connectivity ≠ answer relevance.

### Files modified
- `mcp/src/main.rs` (`generate_context_capsule`)

### Before vs after
```
capsule("websocket metrics streaming for a running simulation")
  BEFORE: pivots = METRICS_QUEUES (empty dict), rx_bytes / network_usage_mb (byte math)
          confidence = "High (Subsystem Validated)"
          supporting context = "No highly relevant supporting context found."
  AFTER:  pivots = simulation_metrics  (the @app.websocket(".../metrics") route — the literal answer),
                   simulation_events
          confidence = "High (Subsystem Validated)" (now justified: the top pivot is the
                       real websocket metrics route)
```

### Remaining limitations
Pivot *recall* for natural-language queries still depends on semantic embeddings (OPENAI_API_KEY at index and query time). With embeddings present, the query now resolves to `simulation_metrics`; without them the capsule degrades to keyword anchors or the existing "no confident matches" abort — both honest failure modes rather than confident-wrong ones.

---

## Bug 5 — `graph_subtree` silently resolves an ambiguous symbol

**Status: Fixed.**

### Root cause
`graph_subtree` resolved its root with `WHERE name = ? LIMIT 1` and the MCP handler had no ambiguity guard (unlike `impact_analysis`/`shortest_path`/`get_implementation`). It silently picked one of two `SOFTWARE_REGISTRY` definitions, accepted no `file_path` to disambiguate, and returned `depth: 0` for a `depth: 2` request with no explanation.

### Architectural fix
- The MCP handler now runs the **same shared `check_symbol_ambiguity`** the other symbol-keyed tools use, and `graph_subtree` gained an optional `file_path` to scope root resolution.
- The response carries a new `truncated_reason` that explains a depth shortfall (e.g. "isolated node, reached depth 0") instead of leaving the caller to guess.

### Files modified
- `query/src/graph.rs` (`graph_subtree` signature, root scoping, `truncated_reason`)
- `mcp/src/main.rs` (ambiguity guard, `file_path` param + schema)

### Before vs after
```
graph_subtree("SOFTWARE_REGISTRY", depth=2)
  BEFORE: silently returns software_registry.py, depth:0, 1 node, no warning
  AFTER:  { ambiguous: true, candidates: [software_registry.py, software_registry_v2.py], hint: ... }
graph_subtree("SOFTWARE_REGISTRY", depth=2, file_path="software_registry_v2")
  AFTER:  resolves v2, truncated_reason: "Root symbol ... has no connected edges
          (isolated node), so the requested depth 2 could not be traversed (reached depth 0)."
```

---

## Bug 6 — `read_file_snippet` returns empty for out-of-range lines

**Status: Fixed.**

### Root cause
Out-of-range `start_line` produced an empty `source` string, indistinguishable from a genuinely blank range.

### Architectural fix
`query::retrieval::read_file_snippet` now validates the range against the real line count and returns an explicit error; it also reports the actually-returned `end_line` (clamped to EOF) when the caller over-asks.

### Files modified
- `query/src/retrieval.rs`

### Before vs after
```
read_file_snippet("main.py", 9000, 9010)
  BEFORE: { source: "" }
  AFTER:  Error: "start_line 9000 exceeds file length (327 lines) for '.../main.py'."
```

---

## Bug 7 — `read_file_skeleton` on a directory gives a generic "not found"

**Status: Fixed.**

### Root cause
`skeletonize_file` returned "File '…' not found in index" before any directory check, and a naive filesystem `is_dir` check failed anyway because the passed segment (`frontend/app`) is nested below the project root (the real directory is `OrcaAI/frontend/app`).

### Architectural fix
When no indexed file matches, the function now treats the input as a **directory segment** and lists the **indexed files that live directly inside it** (more useful than a raw FS listing, since it only names files CodeBroker can actually skeletonize), with a filesystem `is_dir` fallback.

### Files modified
- `query/src/retrieval.rs`

### Before vs after
```
read_file_skeleton("frontend/app")
  BEFORE: "File 'frontend/app' not found in index."
  AFTER:  "'frontend/app' is a directory, not a file. Indexed files in it: layout.tsx, page.tsx.
           Pass one of these as the file path."
```

---

## Additional issues discovered (not in the original report)

### A. The full-index pipeline (`codebroker init`) skipped feature extraction and interaction inference entirely
**Severity: High — and the true reason Bug 1 looked unfixable at first.**
`codebroker init` linked edges and tagged concepts but **never called `infer_interactions` or `extract_features`**, while the incremental `reindex_paths` path did. A freshly `init`-ed repository therefore had an **empty `symbol_features` table**: no PageRank, no fan-in/out, no community ids, no `is_entrypoint` — and no logical interaction edges. Anything reading `symbol_features` (entrypoints, hotspot/centrality ranking, subsystem cohesion) silently degraded until an unrelated incremental reindex happened to populate the table. This is a latent correctness/consistency landmine well beyond Bug 1.
**Fix:** `codebroker init` now runs `infer_interactions` then `extract_features` after linking, making a full index identical in shape to the incremental path. (`cli/src/main.rs`.)
Verified: `symbol_features` went from `0` rows to `443` rows after a clean `init` of netwin.

### B. Hotspot edge filtering was a hardcoded duplicate of the canonical set
`architectural_hotspots` hardcoded `kind IN ('calls','imports','interaction','component_use')` in two queries instead of referencing `CANONICAL_DEPENDENCY_EDGES`. They've been switched to a shared `canonical_edges_sql_list()` helper so the dependency-edge definition lives in exactly one place and `type_ref` is now consistently included everywhere. (`query/src/graph.rs`.)

### C. Capsule base-confidence used a lexicographic `max()` over confidence strings
`max()` over `["High …","Medium …"]` returns the `"Medium"` one (`'M' > 'H'`). Folded into the Bug 4 fix by taking the top-scored pivot's confidence directly.

---

## Regression / "do not regress" verification

Re-checked after the clean reindex; all still correct:

| Tool | Result |
|------|--------|
| `find_symbol` / `impact_analysis` ambiguity (`path`) | ✓ flags 3 candidates with disambiguation hint |
| `get_context` nonexistent symbol | ✓ clean `"Symbol '…' not found in database."` |
| `explore_graph("simulate", outgoing)` | ✓ full accurate chain (compile_topology, FailureEngine, generate_traffic, …); now also includes the real `Topology` type dependency |
| `get_edit_context("run_simulation")` | ✓ accurate forward/reverse deps |
| `dependency_cycles` | ✓ 0 cycles (72 nodes, 100 edges scanned) |
| `search_codebase` semantic fallback | ✓ top-1 for "find attack path between nodes" is `find_path` |

## Build & test gates (all green)
```
cargo fmt           — clean
cargo check --workspace — no errors
cargo test --workspace  — all suites pass (incl. 7 new storage::entrypoints,
                          2 parser type_ref, 1 indexer linker regression test)
cargo build --release   — ok
cargo install --path cli  --force — ok
cargo install --path mcp  --force — ok
```
Then `pkill -f codebroker-mcp` and a fresh `codebroker init` on `/home/labuser/code/netwin` was used to verify every scenario above against the rebuilt binaries.

## Net architectural outcome
The fixes concentrate on shared layers — a single entrypoint classifier, one canonical dependency-edge definition, whole-name graph linking, type-aware parsing, a complete full-index pipeline, and a reused ambiguity guard — so the improvements propagate to *every* tool that consumes them, and the specific failure modes (phantom name-fragment edges, divergent edge bases, silent ambiguity, confidently-wrong labels, empty-on-error reads) are structurally prevented from recurring on future repositories.
