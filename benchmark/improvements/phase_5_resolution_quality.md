# Phase 5: Parser Correctness & Universal Resolution Quality

## Summary

Phase 5 addresses three root causes of poor resolution quality identified from
diagnostics: duplicate symbols from overlapping tree-sitter captures, missing
import classification for stdlib/external packages, and unresolved method calls
due to absent variable→type tracking.

---

## Baseline (before Phase 5)

| Metric | Value |
|--------|-------|
| Import Resolution Success | 70.76% |
| Method Resolution Success | 3.24% |
| Dynamic Fallback Rate | 60.97% |
| Missing Resolution Rate | 15.28% |
| ExternalDependency | 76 |
| StandardLibrary | 0 |

---

## Changes Implemented

### Part 1 — Duplicate Symbol Deduplication

**Problem:** Overlapping tree-sitter queries (e.g., two patterns both matching
`const foo = () => {}`) produced duplicate symbol rows in the DB.

**Fix:** Before calling `insert_symbol`, deduplicate on `(name, kind, start_byte)`
in both `cli/src/main.rs` (full init) and `indexer/src/reindex.rs` (incremental).

### Part 2 — Universal Import Classification

**Problem:** Rust stdlib imports (`use std::collections::HashMap`), Python stdlib
imports (`import os`), and JS/TS external packages (`import React from 'react'`)
were all classified as `Missing` because the resolution pipeline attempted a
symbol-name lookup that found nothing.

**Fixes:**

1. **Rust visitor** (`parser/src/discovery/rust.rs`): `collect_use_leaves` now
   includes the full module path as `source`. `use std::collections::HashMap`
   emits `{name: "HashMap", source: "std::collections"}`.

2. **ClassificationStage** (`indexer/src/resolver/stages/classification.rs`):
   New pipeline stage that runs before name lookup and classifies:
   - `std::` / `core::` / `alloc::` source → `StandardLibrary`
   - `crate::` / `super::` / `self::` source → let repo lookup proceed
   - Rust non-stdlib, non-local source → `ExternalDependency`
   - JS/TS non-relative bare source → `ExternalDependency` (or `Builtin` for Node.js builtins)
   - Python stdlib root module → `StandardLibrary`
   - Python non-relative non-stdlib → `ExternalDependency`
   - Vue/Svelte files treated as JS/TS for external package detection

3. **Pipeline short-circuit**: `ResolutionPipeline::execute` stops early when
   `context.resolved = true`, preventing unnecessary name lookup after classification.

### Part 3 — Receiver & Variable Flow Resolution

**Problem:** Method calls like `db.query()` resolved as `Dynamic` because the
pipeline only did exact name matching (`query`) without knowing the receiver type.

**Fixes:**

1. **Discovery visitors** (TypeScript, Python, Rust): Receiver-aware queries
   emit method_call and new_call/instantiates with `source = receiver_var_name`.
   Run before fallback queries so deduplication keeps the version with receiver info.
   - TypeScript: `obj.method()` → `{name: "method", source: "obj", kind: "method_call"}`
   - TypeScript: `const x = new Foo()` → `{name: "Foo", source: "x", kind: "new_call"}`
   - Python: `db.query()` → `{name: "query", source: "db", kind: "method_call"}`
   - Python: `db = Database()` → `{name: "Database", source: "db", kind: "instantiates"}`
   - Rust: `self.field.method()` → `{name: "method", source: "self", kind: "method_call"}`

2. **SymbolIndex** (`indexer/src/resolver/index.rs`): Added `file_paths`,
   `methods_by_parent` (parent_id → child symbol ids), and `find_method_in_type()`
   for receiver-based method resolution.

3. **Variable map pre-building** (`indexer/src/resolver/mod.rs`): Before the
   linker loop, pre-computes `file_var_map: HashMap<String, String>` per file
   from all `new_call`/`instantiates` relationships where `source` is set.

4. **ReceiverResolutionStage** (`indexer/src/resolver/stages/receiver.rs`):
   New stage that runs before `LexicalGenerationStage`. For method_call with
   a source (receiver variable), looks up the receiver's type in the file var
   map, then calls `find_method_in_type()` to locate the method. If found,
   adds it as a `RepositorySymbol` candidate with `VariableAssignment` evidence.

---

## Results (after Phase 5)

| Metric | Before | After | Δ |
|--------|--------|-------|---|
| Import Resolution Success | 70.76% | 93.82% | +23.1 pp |
| Method Resolution Success | 3.24% | 15.04% | +4.6× |
| Dynamic Fallback Rate | 60.97% | 49.20% | −11.8 pp |
| Missing Resolution Rate | 15.28% | 12.13% | −3.2 pp |
| ExternalDependency | 76 | 393 | +317 |
| StandardLibrary | 0 | 70 | +70 |
| Overall Graph Health | — | 93.8% | — |

### Resolution State Breakdown (after Phase 5)

| State | Count |
|-------|-------|
| Dynamic | 2629 |
| RepositorySymbol | 1155 |
| Missing | 648 |
| Ambiguous | 442 |
| ExternalDependency | 393 |
| StandardLibrary | 70 |
| Builtin | 6 |

---

## Remaining Missing (648)

The remaining Missing relationships are mostly:
- Cross-file relative imports (`../models/Order`) where the exported symbol name
  doesn't match what the importer uses (e.g. aliased types, `export default`)
- Call targets in third-party indexer test fixtures

These require either path-based file resolution (linking `../models/Order.ts`
to its symbols) or export-aware resolution (tracking `export default class Foo`
under the filename as well as the class name). Both are candidates for a
future phase.

---

## Architecture: Pipeline Stages (after Phase 5)

```
[ClassificationStage]   ← stdlib / external detection (short-circuits if matched)
[ReceiverResolutionStage] ← variable→type flow for method calls
[LexicalGenerationStage]  ← exact name lookup in symbol index
[ScopeFilterStage]        ← (stub, future: scope visibility pruning)
[ModuleFilterStage]       ← (stub, future: relative path resolution)
[RankingStage]            ← pick best candidate; assign Dynamic/Missing/Ambiguous
```

### No framework-specific logic

All classification is based on:
- File extension (`.rs`, `.py`, `.ts/.tsx/.js/.jsx/.vue/.svelte`)
- Well-known stdlib module lists (Python, Node.js builtins)
- Module path prefixes (`std::`, `crate::`, `./`, `../`)

No React, Next.js, Flask, Express, Django, or any other framework is
special-cased anywhere in the pipeline.
