# CodeBroker Benchmark — Final Summary

Target repository: `link-up` (Next.js/TypeScript real-time collaborative coding-interview platform, 109 files, 300 symbols, 400 edges)

## Tasks Completed
5/5 — task_001 through task_005, each with a corresponding run_NNN.md report.

| Task | Theme | Overall Score |
|---|---|---|
| run_001 | Full architectural audit | 8/10 |
| run_002 | Real feature implementation (add Dart language support) | 6/10 |
| run_003 | Deep dependency investigation (rooms ↔ problem-engine) | 7/10 |
| run_004 | Refactoring & duplicate logic audit | 5/10 |
| run_005 | Discovery stress test (natural language, no browsing) | 4/10 |

## Average Scores
- **Discovery:** 6.2/10 (7, 6, 8, 6, 4)
- **Search:** 4.5/10 (scored in run_002 and run_005 only: 5, 4)
- **Context:** 6.4/10 (8, 6, 6, 5, 7)
- **Graph:** 8.0/10 (scored in run_001, run_003, run_004: 9, 8, 7)
- **Token Efficiency:** 7.4/10 (9, 7, 9, 7, 5)
- **Overall:** 6.0/10 (8, 6, 7, 5, 4)

## Most Common Failure Types
1. **Opaque negative results.** `shortest_path` (run_003) and `impact_analysis` (run_002, run_004) return `found: false` / empty arrays with no distinction between "genuinely unrelated," "indexing gap," or "related only via a non-static boundary (HTTP/network)." This forced manual verification reads in every task where it came up.
2. **Self-match false positives.** `find_duplicate_logic` (run_004) reported 6 groups where a single function was matched against itself as if it were two duplicate occurrences (identical file_path AND line range) — a real correctness bug, not a coverage gap.
3. **Trusting instructions without structural verification.** `generate_patch` (run_004) complied with a merge instruction asserting two types were "identical" without checking — they weren't (one had an extra optional field). The patch happened to be safe by luck.
4. **Domain-term search misses.** `search_codebase` (run_005) failed on exactly the two most important queries in the discovery stress test — "auth" and "supabase" — missing the actual authentication files entirely despite "supabase" being a literal substring of their paths.
5. **Generic/duplicate AI narrative output.** `project_overview_ai` (run_001) was templated boilerplate; `generate_context_capsule` (run_005) returned byte-identical results for two semantically distinct natural-language queries ("real-time collaboration" vs. "notifications"), suggesting a caching or semantic-discrimination bug.
6. **Missing network/IPC edges.** No tool models `fetch()`-to-API-route relationships as graph edges (run_003), so client↔server subsystem communication is invisible to every graph tool until the route handler is queried directly by name.

## Most Useful Tools Across All Tasks
1. **`graph_subtree`** — single calls reconstructed entire feature subtrees (e.g., 50-node `useRoom` consumer graph) that would otherwise require opening a dozen-plus files.
2. **`architectural_hotspots`** — consistently identified the correct, intuitively-sensible critical symbols (`useRoom`, `createAdminClient`) across both repo-wide and scoped queries.
3. **`shortest_path`** — its negative results were *more* informative than expected once the right connector symbol was found, turning into the single clearest architectural insight of the whole benchmark (run_003's HTTP-boundary finding).
4. **`generate_context_capsule`** — excellent for precise, name-anchored queries (e.g., "add a new programming language wrapper") where it returned full reference implementations plus skeletons in one call; this is the highest token-value tool when the query has literal vocabulary overlap with the codebase.
5. **`find_duplicate_logic`** — best breadth-for-cost discovery tool in the whole suite, despite its self-match bug.

## Least Useful Tools Across All Tasks
1. **`project_overview_ai`** — generic narrative, low marginal value over `project_overview`'s raw stats.
2. **`generate_context_capsule`** for broad, abstract natural-language queries (vs. name-anchored queries) — unreliable and, in one case, produced duplicate/cached-looking output across distinct queries.
3. **`search_codebase`** for domain concepts not literally present in symbol names (auth, supabase) — the single biggest discovery gap found in the entire benchmark.
4. **`generate_patch`** as currently scoped — useful for mechanical changes but dangerously compliant with unverified premises in its instructions.

## Recommended Improvements
1. Make all "not found" / zero-result responses (`shortest_path`, `impact_analysis`, `find_duplicate_logic`) self-describing about *why* nothing was found, including whether the search was exhaustive.
2. Fix the `find_duplicate_logic` self-match bug (dedupe by identity before grouping).
3. Make `generate_patch` verify structural premises (e.g., "these two types are identical") before complying with merge/consolidation instructions, and warn on mismatch.
4. Add a "logical edge" inference pass linking `fetch()` call sites to matching Next.js API route handlers, so client/server subsystem communication is a first-class graph relationship.
5. Improve keyword/path-substring weighting in `search_codebase` so domain terms that appear in directory names (e.g., "supabase," "auth") surface reliably even when they don't appear inside specific symbol names.
6. Investigate and fix the apparent caching/discrimination issue causing `generate_context_capsule` to return identical results for distinct natural-language queries.
7. Trim or better-ground `project_overview_ai`'s narrative output so it adds information beyond what's already inferable from `project_overview`'s deterministic stats.

## Overall CodeBroker Score
**6.0/10** — Strong on graph-based investigation and name-anchored implementation support (especially `graph_subtree`, `architectural_hotspots`, targeted `generate_context_capsule`/`get_edit_context` queries). Weak on natural-language/domain-term discovery and on self-verifying its own outputs before acting (duplicate detection false positives, patch generation trusting unverified premises). In every task, at least one materially important fact was either missed by CodeBroker's tools or required a manual cross-check (reading raw source) to confirm or correct — CodeBroker accelerated discovery in all five tasks but did not fully eliminate the need for manual verification in any of them.
