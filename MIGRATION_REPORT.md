# Universal Resolver Migration Report

## Summary

This report documents every responsibility moved into the Universal Resolver
(`resolver/`) as part of the architectural consolidation described in the
Universal Resolver spec. The goal is a codebase where all symbol resolution,
ambiguity handling, semantic retrieval, lexical retrieval, graph ranking,
confidence scoring, and subsystem seeding happen in exactly one place.

---

## Audit Findings: Pre-Migration State

### What Already Existed in the Universal Resolver

The resolver was partially implemented before this migration. It correctly handled:

| Responsibility | Location |
|---|---|
| Exact/case-insensitive symbol name resolution | `resolver/src/pipeline.rs::stage_exact_symbol`, `stage_canonical_symbol` |
| Semantic embedding resolution (single entity) | `resolver/src/pipeline.rs::stage_semantic_symbol` |
| File / path / directory resolution | `resolver/src/pipeline.rs::stage_full_path`, `stage_filename`, `stage_directory` |
| Domain concept (feature) resolution | `resolver/src/pipeline.rs::stage_feature` |
| Subsystem resolution (confidence gate) | `resolver/src/pipeline.rs::resolve_subsystem` |
| Ambiguity detection and reporting | `resolver/src/types.rs::AmbiguousMatch` |
| Canonical confidence type | `resolver/src/types.rs::Confidence`, `ConfidenceLabel` |
| `NotFound` with stages-tried provenance | `resolver/src/types.rs::NotFound` |

### What Was Duplicated or Bypassed (Pre-Migration)

The following responsibilities were NOT going through the resolver:

#### 1. Concept Augmentation — 3× Duplicated

`search_codebase` inlined its own concept-augmentation loop:
- Looped over `query::concepts::concepts_matching_term(keyword)`
- Called `query::concepts::symbols_for_concept(&db, concept)` for each concept
- Built `SearchResult` entries with `"Concept Match (…)"` confidence label
- Capped at 10 additions, deduped by `(name, path)`

`generate_context_capsule` contained the same conceptual check (via `is_conceptual`) but relied on the subsystem expansion for concept-matched symbols rather than a separate augmentation loop.

#### 2. Initial Candidate Search in `generate_context_capsule` — Bypassed Resolver

`generate_context_capsule` called `query::engine::search_symbols()` directly, bypassing the resolver entirely for its pivot selection. It then applied its own confidence gate (string comparisons against `"Low"`, `"None"`, `"File Path Match"`) instead of using `resolver::types::Confidence`.

#### 3. Subsystem Data Access — Resolver Bypassed for File/Symbol Lists

`generate_context_capsule` called `query::subsystem::discover_subsystem()` directly to obtain the subsystem's file paths, symbol names, and route names for pivot boosting. The resolver's `resolve_subsystem()` existed but returned only `(name, file_count, symbol_count, confidence)` — not the actual membership sets — so callers that needed boosting data had no choice but to bypass the resolver.

#### 4. Semantic Expansion — 3× Duplicated in `mcp/src/main.rs`

The pattern of:
```
getenv OPENAI_API_KEY → OpenAiProvider::new → embed_texts → expand_query
```
was copy-pasted verbatim in three MCP handlers:
- `search_codebase` (lines 1566–1587 pre-migration)
- `subsystem_stats` (lines 2214–2228 pre-migration)
- `generate_context_capsule` handler (lines 2256–2270 pre-migration)

Each independently handled `llm_used`, `query_vector`, and `semantic_tokens` with identical logic and identical error paths.

---

## Changes Made

### `resolver/src/types.rs` — Extended `ResolvedSubsystem`

Added three fields to `ResolvedSubsystem`:

```rust
pub files: Vec<String>,   // absolute file paths in the subsystem
pub symbols: Vec<String>, // core symbol names
pub routes: Vec<String>,  // route/entrypoint symbol names
```

**Why:** Callers that needed subsystem membership sets for boosting (specifically `generate_context_capsule`) previously had to call `discover_subsystem()` directly because the resolver's `ResolvedSubsystem` didn't expose this data. Now the resolver's return type carries everything needed — no bypass required.

---

### `resolver/src/pipeline.rs` — Populated New Fields + Added `resolve_search`

**`resolve_subsystem` and `resolve_any` (subsystem stage):** Updated both sites to populate `files`, `symbols`, `routes` from the `SubsystemStats` returned by `discover_subsystem`. The resolver now owns these data items rather than forcing callers to re-invoke `discover_subsystem`.

**`resolve_search` (new function):** Added a multi-result search entry point that:
1. Delegates to `query::engine::search_symbols()` for lexical + semantic ranking
2. Folds in concept augmentation (`concepts_matching_term` + `symbols_for_concept`)
3. Returns `(Vec<SearchResult>, Option<String>)` sorted by score

This is the single canonical path for "find ranked candidates matching a query." No MCP tool may call `search_symbols` directly or inline its own concept augmentation loop.

---

### `resolver/src/lib.rs` — Exported `resolve_search`

Added `resolve_search` to the crate's public re-export list so MCP tools can call it as `resolver::resolve_search(…)`.

---

### `mcp/src/main.rs` — Three Groups of Changes

#### A. Added `prepare_semantic_context` helper

```rust
fn prepare_semantic_context(query: &str) -> (Vec<String>, Option<Vec<f32>>, bool)
```

Encapsulates the `OPENAI_API_KEY → embed → expand_query` round-trip that was copy-pasted three times. Returns `(semantic_tokens, query_vector, llm_used)`.

Responsibility moved: semantic expansion is now invoked from one definition instead of three identical inline blocks.

#### B. `search_codebase` handler refactored

**Removed:**
- 22-line inline semantic expansion block
- 35-line inline concept augmentation loop (the moved responsibility)
- Nested `match Ok(…)` around the search result

**Replaced with:**
```rust
let (semantic_tokens, query_vector_opt, llm_used) = prepare_semantic_context(keyword);
let (results, reason) = resolver::resolve_search(
    &db, keyword, &semantic_tokens, query_vector_opt.as_deref(),
    llm_used, path_scope, mode, whole_word, min_confidence,
);
```

Handler is now a thin orchestration layer: parse params → call resolver → format output.

#### C. `subsystem_stats` handler refactored

Replaced the 14-line inline semantic expansion block with:
```rust
let (semantic_tokens, query_vector_opt, _) = prepare_semantic_context(name);
```

Updated downstream calls to use `query_vector_opt.as_deref()`.

#### D. `generate_context_capsule` — Initial Search via Resolver

Replaced direct call to `query::engine::search_symbols()` with `resolver::resolve_search()`. The capsule's pivot selection now flows through the same retrieval path as `search_codebase`.

#### E. `generate_context_capsule` — Subsystem Seeding via Resolver

Replaced direct call to `query::subsystem::discover_subsystem()` with `resolver::resolve_subsystem()`. The pivot-boosting logic now uses `sub.files`, `sub.symbols`, and `sub.routes` from `ResolvedSubsystem` rather than bypassing the resolver to reach the raw stats.

`subsystem_confidence` is now derived from `sub.confidence.label` (the typed `ConfidenceLabel` enum) rather than a free-form string.

#### F. `generate_context_capsule` MCP handler refactored

Replaced the 14-line inline semantic expansion block with:
```rust
let (semantic_tokens, query_vector_opt, _) = prepare_semantic_context(query_str);
```

---

## Responsibilities Now in the Universal Resolver

| Responsibility | Resolver Function |
|---|---|
| Exact symbol name resolution | `resolve_symbol` → `stage_exact_symbol` |
| Case-insensitive symbol resolution | `resolve_symbol` → `stage_canonical_symbol` |
| Semantic embedding resolution | `resolve_symbol` → `stage_semantic_symbol` |
| File / path / directory resolution | `resolve_path` → `stage_full_path`, `stage_filename`, `stage_directory` |
| Domain concept resolution | `resolve_any` → `stage_feature` |
| Subsystem resolution + membership data | `resolve_subsystem` |
| Ambiguity detection and reporting | All `resolve_*` functions (via `rows_to_entity`) |
| Confidence scoring (single entity) | `resolver::types::Confidence` |
| Multi-result ranked search | `resolve_search` |
| Concept augmentation | `resolve_search` |

---

## Known Remaining Limitations

The following items were not addressed in this migration pass and are documented here for the next iteration:

### 1. `SearchResult.confidence` Still a String

`query::engine::SearchResult.confidence` is a `String` field (e.g. `"High (Exact Match)"`, `"Medium (Contains Match)"`). The resolver's canonical confidence type is `resolver::types::Confidence`. These two representations are not unified: `generate_context_capsule` still compares `result.confidence.starts_with("Low")` as a string to gate capsule abort.

**Next step:** Change `SearchResult.confidence` to `resolver::types::Confidence`. This would require updating `query::engine::search_symbol_names` to construct `Confidence` objects instead of strings, and updating every caller that reads the string.

### 2. `ContextResponseBuilder::new()` Does Its Own SQL Symbol Lookup

`query::context::ContextResponseBuilder::new()` runs a direct SQL query (`SELECT … WHERE symbols.name = ?1 AND files.path LIKE ?2`) instead of calling `resolver::resolve_symbol()`. This means its ambiguity handling (taking only the first row with `LIMIT 1`) diverges silently from the resolver's `Ambiguous` response.

**Next step:** Refactor `ContextResponseBuilder::new()` to accept a `ResolvedSymbol` from the caller (which the MCP handler already has after calling `resolve_symbol_for_tool`), eliminating the redundant SQL lookup.

### 3. `query::engine::find_symbol()` Re-Implements Matching

The `find_symbol` function in `query/src/engine.rs` independently implements exact/prefix/substring/levenshtein matching — the same logic that `stage_exact_symbol` and `stage_canonical_symbol` implement in the resolver. The `find_symbol` MCP tool uses the resolver for its ambiguity verdict but falls back to `engine::find_symbol` for the ranked result list.

**Next step:** Have `find_symbol` use `resolve_search` for its ranked list, making matching consistent everywhere.

### 4. `subsystem_stats` Still Calls `discover_subsystem` Twice

`subsystem_stats` calls `resolver::resolve_subsystem()` for the confidence gate, then calls `query::subsystem::discover_subsystem()` again for the full stats (`dependencies`, `consumers`, `clusters`, `subsystem_hash`). This is two `discover_subsystem` invocations for one tool call.

**Next step:** Extend `ResolvedSubsystem` with the remaining `SubsystemStats` fields, or expose a `resolve_subsystem_full()` variant that returns everything in one call.

### 5. `skeletonize_file` Has Legacy Path Matching

`query::retrieval::skeletonize_file()` contains its own path-matching fallback (absolute, relative, ends-with). The `read_file_skeleton` MCP tool correctly routes through `resolver::resolve_path()` first, but the inner function's own matching is now redundant and could serve as a confusing fallback.

**Next step:** Remove the path-matching logic from `skeletonize_file` and require callers to pass an already-resolved absolute path.
