# Phase 7 — Graph Resolution & Retrieval Polish

This document outlines the changes made to improve graph correctness, eliminate false positives, and improve retrieval relevance.

## 1. Caller Misattribution Fixed
Previously, the graph linked callers to local variables and assignments, leading to misattribution. Now, the linker determines the enclosing function, method, class, or lambda correctly by filtering out the following variable types from the enclosing symbol logic:
- `variable`
- `constant`
- `parameter`
- `local`
- `property`
- `field`
- `import`

This guarantees that all `CALL` edges originate from valid caller constructs.

## 2. Structural Duplicate Detection
Previously, `find_duplicate_logic` relied on textual whitespace collapsing, which fell prey to noisy differences (e.g. slight formatting deviations, renamed identifiers, variable name differences). 

A new AST Normalizer was added via `parser::normalize::normalize_snippet` utilizing `tree-sitter`. The normalizer masks out specific text values such as:
- Identifiers (`#ID`)
- Strings (`#STR`)
- Numbers (`#NUM`)
- Comments (skipped)

By comparing hashes of structural AST nodes instead of string text, copy-pasted logic with renamed variables is now reliably detected as a duplicate.

## 3. Subsystem Stats Semantic Retrieval
`query/src/subsystem.rs` now properly defers to `crate::engine::search_symbols` for seed generation when looking for components inside a subsystem, relying entirely on semantic relevance rather than simplistic `LIKE` match heuristics.

## 4. Canonical Path Normalization
A critical issue existed where API clients needed to accurately guess whether a file path was absolute, relative, or subsystem-relative. In `query/src/retrieval.rs` (which powers `read_file_skeleton`), path matching is now fully flexible:
- Exact Absolute Matches
- Exact Relative Matches
- Substring (Ends With) Matches

## 5. Reverse Scoring Hierarchy
`search_codebase` now accurately sorts retrieval results using the intended hierarchy:
1. **Semantic Similarity** (Dominant factor, up to 10M base points)
2. **Graph Centrality** (Secondary multiplier for connected nodes)
3. **Keyword Matching** (Tertiary ranking factor)
4. **Path / Location Modifiers** (Quaternary factor)

The ranking naturally bubbles up highly connected, semantically aligned concepts while filtering down incidental noise.

## 6. Testing & Graph Integrity
Two graph-invariant tests were successfully implemented in `mcp/tests/graph_validation_tests.rs`:
- **Caller Kind Invariant**: Ensures zero incoming caller edges originate from `variable`, `parameter`, or `local`.
- **Shortest Path Edge Integrity**: Validates `query::graph::shortest_path` output logic corresponds directly to actual physical edges within the `edges` and `symbols` relations.

**Tests Status**: All `cargo test --workspace` tests passing securely.
**Index Status**: Successfully completely reindexed 206 files.
