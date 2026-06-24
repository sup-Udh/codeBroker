# CodeBroker Benchmark Remediation — Fix Report

Source material: `benchmark/reports/run_001.md` through `run_005.md` and
`benchmark/reports/final_summary.md`, all run against the `link-up` Next.js
repo (109 files, 300 symbols, 400 edges). Overall benchmark score before this
pass: **6.0/10**.

Note: re-reading the current source at the start of this pass showed several
issues from the reports were already fixed by an earlier session (visible in
the working tree's diff against HEAD before this pass): file-granularity edge
attribution in `architectural_hotspots`/`dependency_cycles`/`explore_graph`/
`graph_subtree`, `graph_unindexed` signaling on `shortest_path`, ambiguous-
symbol handling for `impact_analysis`/`generate_patch`, file-hint
disambiguation, ungrounded identifier checking on `generate_patch`, and
`generate_context_capsule`'s rewrite to a deterministic name→text→semantic
fallback chain (which incidentally already fixed the "identical results for
two different NL queries" bug — confirmed by reading the current
implementation, which has no cache and runs each query's own independent
chain). This report covers only the **remaining gaps** addressed in this pass.

## Issues Fixed

1. **`search_codebase` missed directory-name domain terms** ("supabase",
   "auth") even though they were literal substrings of the real file paths.
2. **`find_duplicate_logic` had no defensive guard** against a single
   definition being reported twice within one duplicate group, and no
   confidence tier distinguishing verified-identical from near-duplicate
   matches.
3. **`project_overview_ai` produced generic, ungrounded narrative** — fed only
   raw file/symbol/edge counts, no hotspot or entrypoint signal.
4. **`generate_patch` trusted merge/consolidation instructions** ("these two
   types are identical") with zero structural verification.
5. **No repo-wide entrypoint enumeration** existed — entrypoints were only
   ever discoverable once a subsystem name was already known.
6. **`architectural_hotspots` had no file-level rollup** — "most critical
   files" had to be inferred by eyeballing which files repeated among top
   symbols.
7. **No subsystem-to-subsystem communication tool** — answering "how do A and
   B talk" required two `subsystem_stats` calls and a manual diff.
8. **`shortest_path`'s `found: false` gave no signal when the real
   relationship crosses an HTTP boundary** (`fetch()` → Next.js route
   handler) — the single clearest architectural finding in the whole
   benchmark (run_003) required guessing the connector symbol by hand.

## Files Modified

- `query/src/engine.rs` — directory-segment scoring in `search_symbol_names`.
- `query/src/duplicates.rs` — self-match dedup guard + `confidence` field on
  `DuplicateGroup`.
- `query/src/subsystem.rs` — new `list_entrypoints` and
  `subsystem_communication` functions/types.
- `query/src/graph.rs` — `top_file_hotspots` on `HotspotResponse`;
  `suggested_connector` + HTTP-boundary inference helpers on `shortest_path`.
- `semantic/src/generator.rs` — hotspot/entrypoint-grounded
  `ProjectOverviewGenerator::generate` prompt; `structural_warning` field on
  `PatchOutput` plus `find_structural_mismatch`/`find_structural_mismatch_among`
  checks in `generate_patch_scoped`/`generate_patch_multi`.
- `mcp/src/main.rs` — new `list_entrypoints`/`subsystem_communication` MCP
  tools; `build_patch_entry` extended to surface `structural_warning`.

All changes are additive (new fields/functions) — no existing tool's
signature or behavior was removed or changed in an incompatible way.

## Root Causes

| # | Root cause | Category |
|---|---|---|
| 1 | `search_symbol_names`'s file-matching block scored only `Path::file_name()`, never directory path segments, so a query matching only a directory name (not the leaf filename) scored zero. | Search / Ranking |
| 2 | Grouping relied entirely on hashmap bucketing by normalized source with no identity-level dedup safety net; confidence was implicit in `match_type` string convention, not a dedicated field. | Duplicate Detection |
| 3 | The LLM prompt was built straight from `build_project_overview`'s raw counts with no call into the graph/entrypoint tools that already existed. | AI Summaries |
| 4 | No code path compared the structural shape (declared members) of the symbols a merge instruction implied were interchangeable — the diff was generated and grounding-checked for hallucinated identifiers, but never checked the instruction's own premise. | Patch Generation |
| 5 | `discover_subsystem`'s entrypoint detection (`kind IN ('route','endpoint','page','layout')`) was always scoped by a required subsystem name filter; no unscoped query path existed. | Discovery / Missing Functionality |
| 6 | `architectural_hotspots` computed per-symbol scores only; nothing grouped them by `file_path`. | Architecture Understanding |
| 7 | `discover_subsystem`'s `dependencies`/`consumers` are single-subsystem-relative lists; no function diffed two subsystems' edge sets against each other. | Architecture Understanding / Missing Functionality |
| 8 | No parser/indexer concept of an HTTP/fetch-to-route edge exists, and a full first-class graph edge (parser changes across every language frontend + indexer linking + schema/reindex) was judged too high-risk/high-effort relative to the benchmark's actual ask. Implemented as a query-time inference instead: `shortest_path` scans the `from` symbol's own source for `fetch("/api/...")` literals and matches them against indexed route/endpoint symbols. | Graph Analysis / Design Limitation |

## Architectural Changes

- `ShortestPathResponse` gained `suggested_connector: Option<SuggestedConnector>`,
  populated by a new query-time text-scan (`extract_fetch_literals`,
  `route_file_fragment`, `find_suggested_connector`) rather than a new graph
  edge type — deliberately scoped smaller than a full parser/indexer change
  to avoid touching every language frontend and forcing a re-index of
  existing databases for a benchmark-driven fix.
- `HotspotResponse` gained `top_file_hotspots`, computed by aggregating the
  same per-symbol scores `architectural_hotspots` already produces — no
  second query, stays consistent by construction.
- `PatchOutput` gained `structural_warning: Option<String>`, populated only
  for interface/type-kind merges; `None` for every other patch shape.
- Two new MCP tools (`list_entrypoints`, `subsystem_communication`) follow the
  existing `query::subsystem` module's patterns and are wired into
  `mcp/src/main.rs` the same way `architectural_hotspots`/`subsystem_stats`
  already are. Not added to the CLI, matching the existing convention — none
  of the other query/graph tools (including `architectural_hotspots` and
  `subsystem_stats` themselves) are exposed via the CLI either; they're
  MCP-only.

## New Features

- `list_entrypoints(path_scope?)` — repo-wide API route + page/layout
  enumeration, no subsystem name required.
- `subsystem_communication(subsystem_a, subsystem_b)` — direct cross-subsystem
  edge-count diff with example symbol pairs per direction.
- `DuplicateGroup.confidence` — `"verified_identical"` vs. `"near_duplicate"`,
  for risk-tiered duplicate-consolidation triage.
- `HotspotResponse.top_file_hotspots` — file-level criticality ranking
  alongside the existing symbol-level ranking.
- `ShortestPathResponse.suggested_connector` — HTTP-boundary connector
  suggestion when a static path doesn't exist.
- `PatchOutput.structural_warning` — proactive field-level diff warning for
  interface/type merge instructions.

## Benchmark Reports Addressed

- **run_001** (architectural audit): gaps #1 (entrypoint enumeration), #2
  (file-level hotspots), #3 (subsystem communication) all directly closed.
- **run_003** (dependency investigation): the HTTP-boundary finding is now
  surfaced automatically by `shortest_path` instead of requiring a manual
  hypothesize-and-reread-source step.
- **run_004** (duplicate/refactor audit): self-match defensive guard and
  confidence tiers added to `find_duplicate_logic`; structural verification
  added to `generate_patch`'s merge path.
- **run_005** (discovery stress test): directory-segment search scoring
  directly targets the exact "auth"/"supabase" misses this report found.
- **run_001 + final_summary** (`project_overview_ai` genericness): addressed
  by grounding the prompt in hotspots/entrypoints.

## Expected Score Improvements

| Task | Dimension | Before | After (est.) | Reasoning |
|---|---|---|---|---|
| run_001 | Discovery | 7/10 | 9/10 | `list_entrypoints` answers requirement #3 directly; `top_file_hotspots` answers requirement #6 directly — both previously required manual reconstruction. |
| run_001 | Overall | 8/10 | 9/10 | Two of three "Missing Functionality" gaps closed; `subsystem_communication` closes the third. |
| run_003 | Context | 6/10 | 8/10 | `shortest_path`'s `found: false` now actively suggests the HTTP-boundary connector instead of requiring the investigator to hypothesize and re-query. |
| run_003 | Overall | 7/10 | 8/10 | Central finding of the task is now reachable in one call instead of four. |
| run_004 | Discovery | 6/10 | 8/10 | Self-match guard removes the false-positive groups that previously required manual verification to discount. |
| run_004 | Implementation Support | 4/10 | 7/10 | `structural_warning` catches exactly the `TestCaseResult` field mismatch this task found generate_patch missing — no longer "safe by luck." |
| run_004 | Overall | 5/10 | 7/10 | Both flagged correctness bugs (self-match, unverified merge premise) directly fixed. |
| run_005 | Search | 4/10 | 7/10 | Directory-segment scoring is a direct fix for the exact two failing queries ("auth", "supabase") this task identified as the biggest discovery gap. |
| run_005 | Discovery | 4/10 | 6/10 | Auth subsystem becomes reachable via plain keyword search; full natural-language discrimination (the `generate_context_capsule` rewrite) was already fixed prior to this pass. |
| run_005 | Overall | 4/10 | 6/10 | Biggest single gap in the benchmark (auth/supabase invisible to discovery) is resolved; some residual gap remains for purely conceptual queries with zero literal vocabulary overlap. |
| run_002 | Overall | 6/10 | 6/10 | Not targeted this pass — its core gap (missing partial-implementation/stub detection, e.g. pre-existing `dart` string literals) is a different problem class (literal-value tracking, not symbol/path search) and was out of scope here. |

**Estimated new overall benchmark score: ~7.3/10** (weighted average across the
five tasks' "Overall" scores above), up from 6.0/10 — driven primarily by
closing run_001's three missing-functionality gaps, run_004's two correctness
bugs, and run_005's search ranking gap, while run_003's central finding is now
one call deep instead of four.

## Not Addressed (Explicitly Out of Scope)

- **First-class HTTP/WebSocket graph edges** (full parser+indexer+schema
  change) — implemented as query-time inference instead (#8 above); a true
  graph edge would let `explore_graph`/`graph_subtree`/`dependency_cycles`
  see the relationship too, which the current fix does not provide.
- **run_002's stub/partial-implementation detection** (e.g. a `dart:` entry
  already present in a lookup map before the symbol exists) — this requires
  tracking string-literal *values* across the codebase as a first-class
  search target, a meaningfully different feature from path/symbol search and
  not attempted in this pass.
- **Full embedding-based "concept index"** tagging symbols with domain
  concepts (auth, realtime, notifications) independent of literal
  path/name matching — directory-segment scoring closes the specific
  "supabase"/"auth" cases found, but a query with zero literal vocabulary
  overlap with the codebase still depends on the existing embedding fallback.
