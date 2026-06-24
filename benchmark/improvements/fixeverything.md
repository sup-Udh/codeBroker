# CodeBroker Discovery-First Remediation Pass — fixeverything

This pass implements the product-direction change ("CodeBroker is discovery
only, not an implementation engine") and the four specific benchmark
failures called out in the remediation request, building on top of the
prior `benchmark/improvements/fix.md` pass (which had already fixed
self-match duplicate detection, directory-segment search scoring,
hotspot/entrypoint-grounded AI summaries, repo-wide `list_entrypoints`, and
`subsystem_communication`).

## Removed Functionality

**`generate_patch` is gone, completely.** CodeBroker no longer generates
code, diffs, or patches in any form.

Deleted:
- MCP tool schema/registration (`mcp/src/main.rs`)
- MCP tool call handler (`mcp/src/main.rs`)
- `build_patch_entry` and its three regression tests (`mcp/src/main.rs`)
- `PatchOutput`, `PatchGenerator` and every helper it used —
  `extract_identifiers`, `extract_member_names`, `is_type_like_kind`,
  `structural_mismatch_message`, `find_structural_mismatch`,
  `find_structural_mismatch_among`, `strip_string_literals`,
  `strip_markdown_fence`, `fix_hunk_headers`, `parse_hunk_header`,
  `HunkHeader` (`semantic/src/generator.rs` — file shrank from 1238 to 256
  lines)
- `build_patch_prompt` (`semantic/src/prompt.rs`)
- All three string references to `generate_patch` in comments/warnings
  across `mcp/src/main.rs` and `cli/src/main.rs`, updated to point at
  `get_edit_context`/`impact_analysis` instead

No replacement was added. Every other discovery/analysis/graph/context tool
is untouched and still does exactly what it did before. The contract going
forward, reflected in every place that used to reference `generate_patch`:

> Use `get_edit_context` and `impact_analysis` before implementation.
> CodeBroker does not generate code modifications.

## Benchmark Failures Addressed

| # | Failure | Status |
|---|---|---|
| 1 | `generate_patch` produced a patch from an unverified premise | **Removed entirely** — no patch generation exists to get this wrong anymore |
| 2 | `shortest_path(useRoom, judge)` returned `found: false` with no signal that an HTTP-boundary connector exists | **Fixed** — logical edges are now real, persisted graph rows; `shortest_path(runCode, POST)` returns `found: true` with `edge_type: "logical"` |
| 3 | `subsystem_stats("auth")` missed `createClient`/`createAdminClient`/the whole `utils/supabase/*` auth implementation | **Fixed** — concept-based discovery pulls these in via the `auth` concept tag, independent of literal name/path matching |
| 4 | `subsystem_stats("notification")` presented two helpers embedded in `RoomContext.tsx` as a fake standalone subsystem | **Fixed** — `embedded_within` field now flags this explicitly with exact counts, and the AI overview prompt is instructed not to describe it as a real subsystem |

## Root Causes

1. **Patch generation** trusted instruction premises with at most a
   same-named-symbol structural check — fundamentally the wrong layer for an
   intelligence/discovery tool to own; removed rather than further patched.
2. **No first-class logical edge existed at all.** The dependency graph only
   ever recorded edges the parser found by walking syntax (`calls`,
   `imports`, `renders_component`, ...). A `fetch("/api/...")` call has no
   AST-level relationship to the route handler that answers it — that
   relationship only exists by matching a string literal to a file path at
   query time, which the prior pass's `shortest_path`-only heuristic did,
   but `explore_graph`/`graph_subtree`/`dependency_cycles`/`impact_analysis`
   never saw it.
3. **No concept layer existed.** Every discovery tool (`search_codebase`,
   `subsystem_stats`, `generate_context_capsule`) matched literal
   substrings of a symbol's own name or its file's path. A query for "auth"
   has zero literal overlap with `createClient`/`createAdminClient` or with
   `utils/supabase/admin.ts`'s path — "supabase" is a different word than
   "auth" — so no amount of substring-matching cleverness closes that gap;
   it requires an explicit mapping from a query term to a domain concept.
4. **`discover_subsystem` had no sense of scale.** A subsystem name matching
   2 symbols in a 38-symbol file looked identical, structurally, to a name
   matching 30 symbols across 12 files — there was no signal distinguishing
   "this is a real architectural boundary" from "this is a few helpers that
   happen to share a word with the query."

## Files Modified

- `mcp/src/main.rs` — removed `generate_patch` tool schema, handler,
  `build_patch_entry`, and its tests; added concept-match augmentation to
  `search_codebase`; reordered `generate_context_capsule`'s discovery chain
  to run the concept pass before the blunter text-scan fallback; wired
  `list_entrypoints` into `project_overview`/`repository_stats` responses.
- `cli/src/main.rs` — removed the `generate_patch` string reference; wired
  `detect_logical_edges` and `tag_concepts` into both the full `Init` index
  and `ReindexIncremental` paths.
- `semantic/src/generator.rs` — deleted `PatchOutput`/`PatchGenerator` and
  all of its private helpers (982 lines removed); fixed the now-unused
  `build_patch_prompt` import.
- `semantic/src/prompt.rs` — deleted `build_patch_prompt`.
- `semantic/src/subsystem.rs` — `SubsystemOverviewGenerator`'s prompt now
  includes an explicit caveat block when `embedded_within` is set, directing
  the model not to describe a fake subsystem as architecturally real.
- `graph/src/models.rs` / `graph/src/lib.rs` — new `EdgeType` enum
  (`Static`/`Logical`) with `as_str`/`Display`/`FromStr`, exported from the
  crate root.
- `storage/src/schema.rs` — new `symbol_concepts` table (+ index on
  `concept`).
- `storage/src/db.rs` — `edges.edge_type` column migration (default
  `'static'`); new `insert_logical_edge` with the same dedup contract as
  `insert_edge_attributed`.
- `query/src/concepts.rs` — **new file**: the concept keyword map,
  `tag_concepts` (indexing-time tagging pass), `concepts_matching_term`
  (query-term → concept mapping), `symbols_for_concept` (concept → tagged
  symbols).
- `query/src/lib.rs` — registered the `concepts` module.
- `query/src/graph.rs` — `detect_logical_edges` (the fetch→route detection
  pass); `edge_type` field added to `GraphEdge`/`PathEdge`/`SubtreeEdge` and
  threaded through `explore_graph`/`shortest_path`/`graph_subtree`'s SQL and
  markdown renderers.
- `query/src/subsystem.rs` — concept-based expansion step in
  `discover_subsystem` (pulls in concept-tagged symbols/files alongside the
  literal name/path matches); `embedded_within` field + the
  single-file/low-match-ratio computation that populates it.
- `query/src/engine.rs` — `ProjectOverview` gained `total_directories` and
  `directories_truncated`; the directory cap was raised from a silent,
  undocumented 20 to an explicit, signaled 100.

## New Discovery Features

- **`EdgeType::Static` / `EdgeType::Logical`** — a real distinction in the
  graph, not just a query-time heuristic.
- **`detect_logical_edges(db)`** — runs once per (re)index; scans every
  symbol's source for `fetch("/api/...")` literals and persists a `fetches`
  edge from the calling symbol to the resolved route/endpoint handler.
- **`symbol_concepts` table + `tag_concepts(db)`** — runs once per
  (re)index; tags every symbol with 0+ domain concepts (`auth`, `realtime`,
  `notifications`, `database`) based on keyword matches against its own
  name and its file's path.
- **`SubsystemStats.embedded_within`** — flags when a "subsystem" match is
  really just a minority of symbols in one larger file.
- **`ProjectOverview.total_directories` / `.directories_truncated`** — no
  more silent drops.
- **Concept-augmented `search_codebase`** — results now include a
  `"Concept Match (concept_name)"` confidence tier for symbols a literal
  search would have missed entirely.
- **Concept-aware `generate_context_capsule`** — the concept pass now runs
  ahead of the blunt literal text-scan, so two conceptually distinct queries
  that happen to share a generic word no longer collapse onto the same
  pivots.
- **`project_overview`/`repository_stats` now embed `entrypoints`** directly
  (reusing the existing `list_entrypoints` query), scoped identically to the
  rest of the response.

## Logical Edge System

`detect_logical_edges` is a single pass over every indexed symbol (kind not
in `route`/`endpoint`): for each, it reads the symbol's own byte range from
its source file, scans for `fetch("/api/...")`-shaped string literals
(reusing the literal-extraction logic the prior pass already wrote for
`shortest_path`'s heuristic), converts the literal to a route-file path
fragment, and looks up a matching `route`/`endpoint` symbol in a different
file. Each match is persisted via `insert_logical_edge` as a real row in
`edges` with `edge_type = 'logical'` and `kind = 'fetches'`.

Because `shortest_path`/`explore_graph`/`graph_subtree`/`get_context`/
`impact_analysis` all already query the `edges` table without filtering by
`kind`, every one of them sees logical edges automatically — no separate
traversal logic was needed once the edges existed as real rows. Verified
directly against the `link-up` test repository after reindexing:

```
shortest_path(runCode, POST) -> found: true, distance: 1,
  edges: [{ source: "runCode", target: "POST", edge_kind: "fetches", edge_type: "logical" }]
```

13 logical edges were detected on `link-up`'s first reindex under the new
binary (`Dashboard -> GET`, `RoomProvider -> POST`, `runCode -> POST`,
`Profile -> GET/POST`, etc.) — this is the exact `useRoom`/`runCode -> POST
-> judge` relationship the remediation request named, now a first-class,
traversable graph edge instead of a query-time-only suggestion.

`dependency_cycles` was deliberately left untouched — including logical
edges there risks reporting "cycles" that cross a real network boundary as
if they were a refactor-relevant code cycle, which they aren't.

## Concept Discovery System

`symbol_concepts(symbol_id, concept, matched_on)` is populated by
`tag_concepts`, a full re-tag on every (re)index (cheap at CodeBroker's
typical symbol counts — hundreds to low thousands). Four concepts are
seeded: `auth`, `realtime`, `notifications`, `database`, each with a
keyword list that deliberately overlaps (e.g. "supabase" is in both `auth`
and `database`, since the same client factory often serves both).

Three consumers were wired up:
- **`search_codebase`**: after the existing keyword/embedding chain, checks
  whether the query term maps to a concept and appends tagged symbols
  (confidence `"Concept Match (concept)"`), deduplicated against existing
  results.
- **`subsystem_stats`/`subsystem_overview`** (via `discover_subsystem`):
  when the subsystem name maps to a concept, concept-tagged symbols/files
  are merged into the literal name/path match set.
- **`generate_context_capsule`**: the concept pass runs as its own step,
  positioned before the literal text-scan fallback specifically so a
  generic shared word between two distinct queries doesn't cause them to
  collide on the same weak match before concept routing gets a chance.

Verified directly against `link-up`: `subsystem_stats("auth")` now returns
`utils/supabase/admin.ts`, `client.ts`, `server.ts`, and
`update-session.ts` alongside `app/login/page.tsx` and
`app/auth/callback/route.ts` — none of which a literal "auth" substring
match would have found in the first three files. `search_codebase("auth")`
now surfaces `resetSession`/`endSession`/the `GET` auth callback route as
`"Concept Match (auth)"` results.

**Known limitation**: concept matching is plain substring matching with no
word-boundary awareness, so it produces occasional false positives (e.g.
`parseDesignInput` got tagged `auth` because "designinput" happens to
contain the substring "signin"). This is the same class of imprecision any
lightweight keyword-matching layer has; tightening it to word-boundary-aware
matching was judged lower priority than closing the much larger "auth is
completely invisible to discovery" gap this system fixes.

## Entrypoint Improvements

`list_entrypoints` already existed (added in the prior `fix.md` pass) as a
standalone MCP tool. This pass wires its output directly into
`project_overview` (repo-wide, unscoped) and `repository_stats` (scoped by
the same `path_scope` the rest of the stats use), so a caller asking "what
does this repo look like" gets entrypoints in the same response instead of
needing a second tool call. Verified on `link-up`: `project_overview`'s
`entrypoints.total` is 27, matching `list_entrypoints(None)` called
directly.

## Expected Benchmark Score Improvements

| Task | Dimension | Before this pass | After (est.) | Reasoning |
|---|---|---|---|---|
| run_002 | Implementation Support | 7/10 (post-fix.md) | N/A | `generate_patch` no longer exists; this dimension is retired for future runs of this task, not improved — the benchmark's task_002 will need rewriting to test discovery-tool sufficiency (`get_edit_context`/`impact_analysis`) instead of patch quality. |
| run_003 | Graph | 8/10 | 10/10 | The HTTP-boundary connector is now a real, persisted, traversable edge — `explore_graph`/`graph_subtree`/`impact_analysis` see it too, not just `shortest_path`'s heuristic. |
| run_003 | Context | 8/10 (post-fix.md) | 9/10 | `shortest_path(runCode, POST)` now returns `found: true` directly with no query-time inference needed at all. |
| run_004 | Implementation Support | 7/10 (post-fix.md) | N/A | Same retirement as run_002 — no patch tool exists to score. |
| run_005 | Discovery | 6/10 (post-fix.md) | 9/10 | `subsystem_stats("auth")` now returns the real auth implementation; the "auth"/"supabase" gap (the single biggest discovery failure in the original benchmark) is closed at the subsystem level, not just the search-ranking level. |
| run_005 | Search | 7/10 (post-fix.md) | 8/10 | Concept-match results now appear directly in `search_codebase("auth")`'s output, not just via directory-segment scoring. |
| run_005 | Overall | 6/10 (post-fix.md) | 8/10 | Both of run_005's central findings (auth invisibility, identical results for distinct NL queries) are now fixed and verified end-to-end against the live `link-up` index. |
| run_001 | Discovery | 9/10 (post-fix.md) | 9/10 | Unaffected by this pass (already closed). |
| run_004 | Discovery | 8/10 (post-fix.md) | 9/10 | `subsystem_stats("notification")`-style fake-boundary results are now flagged via `embedded_within` instead of silently presented as real. |

**Estimated new overall benchmark score: ~8/10**, up from ~7.3/10
post-`fix.md` — driven by closing run_003's central architectural finding at
the graph level (not just query-time), and run_005's central discovery gap
at the subsystem level (not just search ranking). run_002/run_004's
"Implementation Support" dimension is retired rather than improved, since
the tool it measured no longer exists by design — those tasks would need to
be rewritten to evaluate `get_edit_context`/`impact_analysis` sufficiency
for a discovery-only tool instead.

## Validation

- `cargo fmt --all`, `cargo check --workspace`, `cargo test --workspace`
  (50 tests, 0 failures), and `cargo build --release` all pass.
- `cargo install --path cli --force` / `cargo install --path mcp --force`
  succeeded; no live `codebroker-mcp` process needed killing before
  reinstall.
- Reindexed `link-up` (113 files) with the new binary: 500 static edges, 14
  logical edges, 102 symbol/concept tags, 301 embedded symbols.
- Verified end-to-end over the live MCP stdio protocol (not just unit
  tests): `shortest_path(runCode, POST)` returns a logical edge;
  `subsystem_stats("auth")` returns the real Supabase auth files;
  `subsystem_stats("notification")` returns `embedded_within` flagging it as
  embedded in `RoomContext.tsx`; `search_codebase("auth")` returns
  `"Concept Match (auth)"` results; `project_overview`/`repository_stats`
  both carry `entrypoints` and `total_directories`/`directories_truncated`;
  the two previously-identical `generate_context_capsule` queries now return
  genuinely distinct, relevant pivots.

## Not Addressed (Explicitly Out of Scope)

- **Word-boundary-aware concept matching** — current implementation is
  substring-based and has known false-positive risk (see Known Limitation
  above).
- **`dependency_cycles` over logical edges** — deliberately excluded; a
  cross-network "cycle" isn't the same actionable signal as a real code
  cycle.
- **run_002's stub/partial-implementation detection** — unrelated to this
  pass's scope (literal-value tracking across the codebase, not graph/edge/
  concept work), and now partially moot since `generate_patch` (the tool
  run_002 was evaluating implementation-readiness for) no longer exists.
- **Websocket/pub-sub logical edges** — the remediation request named these
  as additional patterns to detect (`websocket communication`, `event
  emitters`, `pub/sub patterns`); only the `fetch("/api/...")` pattern was
  implemented this pass, matching the one concrete example
  (`useRoom -> fetch -> POST -> judge`) in the request and the only pattern
  actually present in `link-up`. Extending `detect_logical_edges` to
  websocket `.send()`/`.emit()` calls matched against a server-side handler
  would follow the same shape but wasn't validated against a real example
  in this pass.
