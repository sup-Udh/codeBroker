# Phase 9 — Retrieval Precision & Subsystem Intelligence

## 1. Root Cause Analysis

### A. search_codebase Failures
**Current Behavior:** Natural language queries return noisy results (variables, generic helpers, tests) over production implementations. Semantic search seems weak compared to lexical matching.
**Root Cause:**
1. **Scoring Scaling Issues:** In `engine.rs`, the final ranking formula uses raw multiplicative weights: `(semantic_score * 100_000) + (graph_score * 10_000) + (name_score * 100) + path_score`. Because `semantic_score` is derived directly from cosine similarity (which has a narrow range of variation, typically 0.8-1.0), the massive 100,000 multiplier causes semantic noise (e.g. a slightly higher cosine similarity for `auth_pct` vs `authenticate()`) to irreversibly overshadow structural and graph features.
2. **Lack of Tiered Tie-Breaking:** When multiple symbols share a similar semantic similarity or lexical prefix, there is no discrete tie-breaking based on architectural importance (e.g., choosing the `is_entrypoint` or higher `pagerank` over a generic local variable). 
3. **Overly Broad Semantic Retrieval:** All symbol kinds are embedded, so variables and temporary helpers are just as likely to trigger semantic hits if their names align with the embedding query.

### B. subsystem_stats Failures
**Current Behavior:** Subsystem discovery struggles with precision, ambiguous names, and continues expanding beyond cohesive boundaries.
**Root Cause:**
1. **Ambiguous Seeding:** `discover_subsystem` relies on `search_symbols`. If the query is "User", it retrieves every `User` symbol across the codebase (frontend component, backend ORM model, protobuf definition). It dumps all of these IDs into a seed pool.
2. **Naive Community Dominance:** It determines the subsystem by finding the most common `community_id` among the noisy seed pool. If frontend files happen to have more symbols named "User", the frontend community is incorrectly chosen.
3. **Unbounded Expansion:** Once the dominant `community_id` is found, it unconditionally queries `SELECT symbol_id FROM symbol_features WHERE community_id = X AND is_local = 0`. Label propagation often creates large macro-communities in densely connected graphs. Expanding unconditionally to the entire community ignores local relevance, returning vast swathes of the repo instead of a tight, context-specific subsystem.

## 2. Retrieval Pipeline Redesign

We will implement a unified retrieval pipeline in `engine.rs` that all MCP tools use.

**New Pipeline Architecture:**
1. **Candidate Generation (Broad):** Fetch candidates via lexical matching and semantic search (cosine similarity > threshold).
2. **Ambiguity Resolution (Clustering):** Group candidates by exact name match. If ambiguity exists (e.g., 3 symbols named `validate`), we calculate a contextual disambiguation score for each based on:
   - `pagerank` (Higher centrality = more likely the canonical implementation)
   - `is_callable` / `is_type` vs `is_local`
   - Community density (Does this candidate share a community with other top semantic hits for the query?)
3. **Community-Aware Reranking (Cross-Scoring):** We apply a non-linear scoring system instead of raw multipliers. 
   - Base Score = Semantic (0-100) + Lexical (0-100).
   - Multipliers: Structural importance scales the base score (e.g. `is_entrypoint` = 1.5x, `is_local` = 0.5x, `pagerank` scales 1.0x - 2.0x).
4. **Final Ranking:** Sort by the normalized final score.

## 3. Subsystem Intelligence (Community Expansion)

**Redesigning `discover_subsystem`:**
1. **Canonical Seed Selection:** Use the new retrieval pipeline to get candidates. Apply ambiguity resolution to pick the *single* highest-confidence canonical seed for the query (or a very tight cluster of top 3).
2. **Cohesion-Bounded Expansion:** Instead of returning the entire `community_id`, we perform a bounded Random Walk or Personalized PageRank starting from the canonical seed(s). We expand outwards along edges, stopping when the visit probability drops below a threshold. This guarantees we only return the tightly coupled structural neighborhood rather than the whole macro-community.

## 4. Proposed Changes

### `query/src/engine.rs`
- **[MODIFY]** Rewrite `search_symbol_names` to implement the structured reranking and scoring multipliers.
- **[NEW]** Add an `ambiguity_resolution` phase that groups identical symbol names and suppresses the weaker instances (locals, low pagerank) so only the canonical definition competes in the final ranking.

### `query/src/subsystem.rs`
- **[MODIFY]** Rewrite `discover_subsystem`. Instead of `WHERE community_id = X`, implement a weighted traversal (e.g., localized expansion) originating from the top disambiguated seed symbol.

### `indexer/src/features.rs`
- **[MODIFY]** Enhance Label Propagation to penalize large, monolithic communities (optional, if bounded expansion is sufficient we may just rely on the traversal).

## 5. User Review Required

Does this pipeline align with your vision for ambiguity resolution and subsystem cohesion? Specifically:
1. **Ambiguity Resolution:** Should we filter out weaker ambiguous symbols completely, or just heavily penalize them so they appear lower in the search results?
2. **Subsystem Bounding:** I plan to use a Localized Graph Traversal (expanding only to nodes with high edge-density to the seed) rather than dumping the whole `community_id`. Does this sound right?
