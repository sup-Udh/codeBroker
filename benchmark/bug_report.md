# CodeBroker Bug Report & Fix Brief

**Audience:** an engineer/agent implementing fixes to the CodeBroker MCP server.
**Test repo:** `/home/labuser/code/netwin` (OrcaAI — FastAPI orchestrator + Next.js/React frontend, network-topology simulator). DB at `.codebroker/codebroker.db`, 47 files, 443 symbols, 145 edges at time of testing.

All issues below were reproduced live against this repo's MCP tool calls. Each entry has: the exact call made, expected vs. actual output, root-cause hypothesis, and a concrete fix suggestion. Re-verify against current `main` before fixing, since the index may have changed.

---

## Bug 1 — Entrypoint/route detection returns zero for every framework in this repo

**Severity: High.** This is the single most damaging bug — it silently returns an empty, confident-looking result instead of erroring, so a caller has no signal that anything is wrong.

**Repro A (FastAPI):**
```
project_overview()  -> entrypoints: { pages: [], routes: [], total: 0 }
list_entrypoints()  -> { total: 0, routes: [], pages: [] }
repository_stats(path_scope="orchestrator") -> same, total: 0
```
But `OrcaAI/orchestrator/main.py` has 8 real FastAPI routes:
```python
@app.get("/")
@app.post("/analyze-topology")
@app.get("/supported-software")
@app.get("/simulation/{simulation_id}/failures")
@app.get("/simulation/{simulation_id}/bottlenecks")
@app.websocket("/simulation/{simulation_id}/events")
@app.websocket("/simulation/{simulation_id}/metrics")
@app.post("/simulate")
```
Plus `OrcaAI/orchestrator/import_engine/import_service.py`:
```python
@router.post("/import-topology")
def import_topology(file: UploadFile = File(...)): ...
```

**Proof the decorator data IS parsed** (so this isn't a tokenizer gap, it's an aggregation gap):
```
get_context(symbol="import_topology")
-> "signature": "[@router.post(\"/import-topology\")] def import_topology(file: UploadFile = File(...))"
```
`read_file_skeleton("OrcaAI/orchestrator/main.py")` also renders every `@app.get/post/websocket(...)` decorator inline above each function. The decorator string is captured at the symbol level — it's just never promoted into the `entrypoints`/`routes` aggregate.

**Repro B (Next.js):**
```
repository_stats(path_scope="frontend") -> entrypoints: { pages: [], routes: [], total: 0 }
```
But `OrcaAI/frontend/app/page.tsx` and `OrcaAI/frontend/app/layout.tsx` exist and are textbook Next.js App Router entrypoints (confirmed via direct file read — `page.tsx` default-exports `Home()`).

**Root cause hypothesis:** the entrypoint aggregator that backs `project_overview.entrypoints` / `list_entrypoints` / `repository_stats(...).entrypoints` appears to be a no-op or matching on a convention that doesn't fire for either (a) Python decorator-based routes (`@app.get`, `@router.post`, `@app.websocket`) or (b) Next.js App Router file convention (`app/**/page.tsx`, `app/**/layout.tsx`). It's possible it only matches Pages Router (`pages/**/*.tsx`) or some other now-incorrect glob.

**Fix suggestions:**
1. For Python: scan symbol `signature`/decorator metadata already captured (visible via `get_context`) for `@app.get|post|put|delete|websocket|patch(...)` and `@router.<method>(...)`, and surface matches in `routes`.
2. For Next.js: detect `app/**/page.{tsx,jsx,ts,js}` and `app/**/layout.{tsx,jsx,ts,js}` (App Router) in addition to whatever `pages/**/*.tsx` (Pages Router) logic currently exists, and surface in `pages`.
3. Add a regression test repo fixture with both conventions present so this can't silently regress again — the fact that it returns `0` instead of erroring is what makes it dangerous.

---

## Bug 2 — Reverse-dependency / impact-analysis edges keyed on parameter name, not resolved type

**Severity: High.** This is a correctness bug in the core "what depends on this?" feature — exactly the kind of answer someone would trust before refactoring.

**Repro:**
```
impact_analysis(symbol="Topology", format="structured")
-> "reverse_dependencies": ["analyze_topology"]

get_context(symbol="Topology", file_path="main.py")
-> "reverse_dependencies": ["analyze_topology"]   (same wrong answer)
```

But in `OrcaAI/orchestrator/main.py`:
```python
class Topology(BaseModel): ...

@app.post("/analyze-topology")
def analyze_topology(topology: dict): ...      # <-- takes a plain dict, NOT Topology

@app.post("/simulate")
def simulate(topology: Topology): ...          # <-- actually typed as Topology, but MISSING from reverse_dependencies
```

So the tool reports a dependency edge to the function that does *not* use the type, and omits the edge to the function that does. This strongly suggests the edge-builder is matching on the *parameter identifier name* (`topology`) across the codebase rather than resolving the annotation to the actual `Topology` class.

**Fix suggestion:** when building reverse-dependency/forward-dependency edges for a class used as a type annotation, resolve via the annotation's resolved type (AST-level type identity), not by string-matching the parameter/variable name. Add a unit test: two functions with a same-named parameter but different/no type annotation must not both register as dependents of the class.

---

## Bug 3 — `subsystem_communication` contradicts `subsystem_stats` on the same edge set

**Severity: Medium-High.** Two tools give mutually exclusive answers to the same factual question, with no way for a caller to know which is right.

**Repro:**
```
subsystem_stats(subsystem_name="orchestrator")
-> "consumers": [
     "./OrcaAI/frontend/src/components/NodeConfigPanel.tsx",
     "./OrcaAI/orchestrator/tests/test_import_pipeline.py"
   ]

subsystem_communication(subsystem_a="orchestrator", subsystem_b="frontend")
-> { "a_to_b_edges": 0, "b_to_a_edges": 0, "a_to_b_examples": [], "b_to_a_examples": [] }
```

If `NodeConfigPanel.tsx` (under `frontend`) is genuinely a consumer of `orchestrator` symbols, `subsystem_communication` should show at least one `b_to_a` edge (frontend -> orchestrator). It shows zero in both directions.

**Root cause hypothesis:** `subsystem_stats`'s "consumers" list and `subsystem_communication`'s edge diff likely use different underlying queries/edge-kind filters (e.g. one includes file-level "referenced by" heuristics the other excludes, or one of the two subsystem-name matches is resolving to a different file set than intended — e.g. "frontend" substring matching is ambiguous between `OrcaAI/frontend` top dir vs `OrcaAI/frontend/src/components`).

**Fix suggestion:** make both tools call the same underlying edge-resolution function so they can't diverge. Add a test asserting `subsystem_stats(A).consumers` and `subsystem_communication(A, B)` agree on whether any A<->B edge exists.

---

## Bug 4 — `generate_context_capsule` returns confidently-wrong, high-confidence results on a clearly-answerable query

**Severity: High.** This is the most "hallucination-like" behavior found: the tool asserts high confidence while missing the obviously correct answer.

**Repro:**
```
generate_context_capsule(query="websocket metrics streaming for a running simulation")
```
**Actual output:**
- Confidence label: `"High (Subsystem Validated)"`
- Pivot symbols returned: `METRICS_QUEUES` (an empty dict declaration), and `network_usage_mb` / `rx_bytes` (unrelated byte-math variables inside `bottleneck_engine.py`)
- Supporting context section: `"No highly relevant supporting context found."`

**What it should have found** (all present and well-connected in the graph, confirmed via other tools in the same session):
- `stream_metrics()` in `OrcaAI/orchestrator/metrics_engine.py` — directly named after "metrics streaming"
- `emit_ws_event()` in `OrcaAI/orchestrator/simulation_engine.py` — the actual websocket-emit function
- The `@app.websocket("/simulation/{simulation_id}/metrics")` route (`simulation_metrics`) in `main.py` — the literal websocket entrypoint for metrics
- `explore_graph(symbol="simulate", direction="outgoing")` from earlier in this session shows `simulate -> stream_metrics` and `stream_metrics -> get_container_metrics -> detect_bottlenecks` as a clean, directly relevant chain — proving the graph data needed to answer this query correctly does exist and is reachable.

**Root cause hypothesis:** the capsule's keyword/semantic anchor selection latched onto `METRICS_QUEUES` (literal token overlap with "metrics") and then expanded from a low-relevance neighborhood (`bottleneck_engine.py`) instead of `metrics_engine.py` or the websocket route symbols, and the "Subsystem Validated" confidence label appears to be a structural/graph-connectivity check rather than a semantic-relevance check — i.e. it validates "these symbols are in a connected subsystem" without validating "these symbols actually answer the query." That mismatch is what makes the confidence label misleading.

**Fix suggestions:**
1. Don't let purely lexical anchor matches (e.g. matching "metrics" against `METRICS_QUEUES`) outrank semantically central symbols like `stream_metrics`/`emit_ws_event` — weight function/route symbols higher than bare variable declarations when both are candidates.
2. Decouple "Subsystem Validated" (graph-connectivity) from an actual relevance/confidence signal, or rename the label so callers don't mistake structural validation for answer-quality validation.
3. When the query contains words that exactly match a function name (`stream_metrics` ~ "metrics streaming"), that should be a strong prior the current pipeline seems to be discarding.

---

## Bug 5 — `graph_subtree` silently resolves an ambiguous symbol instead of flagging ambiguity, giving a misleading "isolated symbol" result

**Severity: Medium.** Inconsistent behavior across tools turns an ambiguity case into a silent wrong answer.

**Repro:**
```
find_symbol(symbol="SOFTWARE_REGISTRY")
-> two exact matches, both score 1000:
   - OrcaAI/orchestrator/software_registry.py (line 1)
   - OrcaAI/orchestrator/software_registry_v2.py (line 8)

graph_subtree(root_symbol="SOFTWARE_REGISTRY", depth=2)
-> { "depth": 0, "node_count": 1, "edge_count": 0, "edges": [],
     "nodes": [["SOFTWARE_REGISTRY", "variable", "./OrcaAI/orchestrator/software_registry.py"]] }
```
It silently picked `software_registry.py` (the file with **no** confirmed consumers in this repo) and reported it as a fully isolated symbol with 0 edges at depth 0 — even though `depth=2` was requested. Meanwhile `main.py` actually does:
```python
from software_registry_v2 import SOFTWARE_REGISTRY
```
i.e. the symbol that's actually wired into the app is in `software_registry_v2.py`, not the one `graph_subtree` silently chose.

**Contrast with correct ambiguity handling elsewhere:**
```
impact_analysis(symbol="path")
-> { "ambiguous": true, "candidates": [...3 files...],
     "hint": "Multiple symbols share this name. Re-run with `file_path` set..." }
```
`impact_analysis` and `find_symbol` both do the right thing (either return all candidates, or flag ambiguity and ask for disambiguation via `file_path`). `graph_subtree` has no such guard and just silently uses one candidate with no warning, no `ambiguous` flag, and a returned `depth: 0` that doesn't match the requested `depth: 2` (no explanation/warning for that mismatch either).

**Fix suggestions:**
1. Add the same ambiguity check used in `impact_analysis`/`get_context` to `graph_subtree` (and audit other symbol-name-keyed tools for the same gap — `explore_graph`, `read_symbol_source`, `get_edit_context` should all be checked).
2. When `depth` requested != `depth` achieved, surface why (e.g. `"truncated_reason": "root symbol has 0 outgoing/incoming edges"`) instead of silently returning a lower depth.

---

## Bug 6 — `read_file_snippet` returns an empty string instead of an error for out-of-range line numbers

**Severity: Low-Medium.** Silent failure mode that could mislead a caller into thinking a file region is empty.

**Repro:**
```
read_file_snippet(path="OrcaAI/orchestrator/main.py", start_line=9000, end_line=9010)
-> { "file_path": ".../main.py", "start_line": 9000, "end_line": 9010, "source": "" }
```
`main.py` is nowhere near 9000 lines. No error, no warning — just an empty `source` field, indistinguishable from "this range really is blank lines."

**Fix suggestion:** validate `start_line`/`end_line` against the actual file's line count and return an explicit error (or a `"warning": "start_line exceeds file length (N lines)"` field) rather than silently returning empty content.

---

## Bug 7 (minor/UX) — `read_file_skeleton` on a directory path gives a generic "not found" error

**Severity: Low.** Not incorrect, just a poor error message.

**Repro:**
```
read_file_skeleton(file_path="frontend/app")
-> "Error reading file skeleton: File 'frontend/app' not found in index."
```
`frontend/app` is a real directory (contains `page.tsx`, `layout.tsx`, `globals.css`, `favicon.ico`), but the tool only accepts file paths, and the error message doesn't distinguish "doesn't exist" from "is a directory, did you mean one of its files?"

**Fix suggestion:** if the path substring matches a directory, return a helpful error listing the files inside it (similar to how `find_symbol` lists candidates) instead of a generic not-found.

---

## What worked correctly (do not regress these)

- `find_symbol` — correctly surfaces all ambiguous matches with scores, no silent guessing.
- `impact_analysis` — correctly detects and flags ambiguous symbol names with actionable disambiguation hints (`path` example above).
- `get_context` — clean, correct error for nonexistent symbols (`"Symbol 'x' not found in database."`).
- `explore_graph` — produced an accurate, correct call/dataflow tree for `simulate` (compile_topology → find_path → run_simulation → stream_metrics) that matched the real code exactly. Use this as the reference implementation when fixing Bug 4's capsule relevance ranking.
- `get_edit_context` — accurate full source + forward/reverse deps for `run_simulation`.
- `dependency_cycles` — ran cleanly, reported 0 cycles (consistent with the small/non-recursive nature of this codebase).
- `search_codebase` semantic fallback — top-ranked result for a natural-language query about attack-path-finding correctly returned `find_path` in `attack_engine.py`. (Separately flagged in this session as noisy/unbounded — consider a `limit`/`kind` filter — but the top-1 relevance was good.)

---

## Suggested priority order for fixes

1. **Bug 1** (entrypoint detection returns 0 for every framework) — highest blast radius, affects the most basic "what does this app expose" question.
2. **Bug 4** (capsule hallucinated high-confidence wrong answer) — most dangerous because it actively misleads with a confidence label.
3. **Bug 2** (Topology reverse-dependency mismatch) — undermines trust in impact analysis specifically.
4. **Bug 3** (subsystem_communication vs subsystem_stats contradiction) — internal consistency.
5. **Bug 5** (graph_subtree silent ambiguity) — consistency fix, reuse existing ambiguity-detection code from impact_analysis.
6. **Bugs 6 & 7** — minor error-handling polish.