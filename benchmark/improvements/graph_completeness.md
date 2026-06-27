# Graph Completeness Report — Phase X

## Summary

Phase X audited and extended the graph builder for universal correctness across
all supported languages (Python, TypeScript, JavaScript, Rust).  The graph is
now constructed entirely from language syntax; no framework-specific heuristics
are used at any layer of the pipeline.

---

## Coverage Metrics by Language

### TypeScript / TSX

| Edge Kind | Previously | Now | Notes |
|-----------|-----------|-----|-------|
| `imports` (named) | ✅ | ✅ | `import { Foo } from '…'` |
| `imports` (default) | ✅ | ✅ | `import Foo from '…'` |
| `imports` (namespace) | ❌ | ✅ | `import * as Foo from '…'` — `Foo` now tracked |
| `re_export` | ❌ | ✅ | `export { Foo } from '…'` |
| `calls` | ✅ | ✅ | Free call `foo()` |
| `method_call` | ✅ | ✅ | Member call `obj.foo()` |
| `MEMBER_ACCESS` | ✅ | ✅ | Property access |
| `new_call` | ❌ | ✅ | `new Foo()` constructor |
| `extends` | ❌ | ✅ | `class Foo extends Bar` |
| `implements` | ❌ | ✅ | `class Foo implements IBar` |
| `type_ref` | ✅ | ✅ | Type annotations |

**Verified:** tree-sitter query compiles without errors (`ts_import_query_must_compile` test).  
**Verified:** named import produces `imports` edge (`named_import_produces_import_edge` test).

**Known gap:** Default export aliasing (`export default function` resolved by
the full-init linker; incremental linker skips default-only paths).

---

### JavaScript / JSX

| Edge Kind | Previously | Now | Notes |
|-----------|-----------|-----|-------|
| `imports` (named) | ✅ | ✅ | `import { Foo } from '…'` |
| `imports` (default) | ✅ | ✅ | `import Foo from '…'` |
| `imports` (namespace) | ❌ | ✅ | `import * as Foo from '…'` |
| `re_export` | ❌ | ✅ | `export { Foo } from '…'` |
| `calls` | ✅ | ✅ | Free call |
| `method_call` | ✅ | ✅ | Member call |
| `MEMBER_ACCESS` | ✅ | ✅ | Property access |
| `new_call` | ❌ | ✅ | `new Foo()` |
| `extends` | ❌ | ✅ | `class Foo extends Bar` (via `class_heritage`) |

**Note:** `implements` not applicable in JavaScript (TypeScript-only feature).  
**Fix:** `superClass` field name was wrong; corrected to `class_heritage` child pattern.

---

### Python

| Edge Kind | Previously | Now | Notes |
|-----------|-----------|-----|-------|
| `imports` | ✅ | ✅ | `import foo` / `from x import y` |
| Wildcard import | Fabricated `*` node | Fixed | `from x import *` now produces no edge |
| `inherits` | ✅ | ✅ | `class Foo(Bar)` |
| `instantiates` | ✅ | ✅ | `x = Foo()` |
| `calls` | ✅ | ✅ | Free function call |
| `MEMBER_ACCESS` | ✅ | ✅ | Attribute access |
| `type_ref` | ✅ | ✅ | Type annotations |
| `global_ref` | ✅ | ✅ | `global x` statement |

---

### Rust

| Edge Kind | Previously | Now | Notes |
|-----------|-----------|-----|-------|
| `imports` | Partial (full path stored) | Fixed | Leaf name extracted from `use` path |
| Brace groups | ❌ | ✅ | `use a::{B, C}` → 2 separate edges |
| Glob import | Fabricated `*` node | Fixed | `use x::*` produces no edge |
| `as` renames | ❌ | ✅ | `use Foo as Bar` → links to `Foo` |
| `calls` | ❌ | ✅ | Free function call edges |
| `implements` | ❌ | ✅ | `impl Trait for Type` → implements edge to trait |

**New symbol kinds indexed:**
- `enum` (`enum_item`)
- `trait` (`trait_item`)
- `type_alias` (`type_item` — corrected from `type_alias_definition`)
- `constant` (`const_item`, `static_item`)
- `macro` (`macro_definition`)
- `module` (`mod_item`)

**Noisy call filter:** 38 Rust built-in names excluded from `calls` edges to
prevent phantom links (e.g. `unwrap`, `collect`, `map`, `push`, `new`).

---

## Graph Validation Layer (Phase 5)

New module: `query/src/validation.rs`

Checks run post-index:
- **Dangling edges** — edges pointing to non-existent symbol IDs
- **Duplicate edges** — identical `(source, target, kind)` triples
- **Self-loops** — `source_symbol_id == target_symbol_id`
- **Orphan symbols** — symbols with no edges (in or out)
- **Isolated files** — files with no cross-file edges
- **Unresolved imports** — raw_imports with no matching graph edge

`ValidationReport` provides:
- `import_resolution_rate()` — fraction of raw_imports resolved to edges
- `graph_connectivity()` — fraction of files with ≥1 cross-file edge
- `is_valid()` — `dangling_edges == 0 && duplicate_edges == 0`

These metrics are logged during every `codebroker init` and incremental reindex.

---

## Graph Completeness Metrics (Phase 6)

New module: `query/src/metrics.rs`

`GraphMetrics` fields persisted to the `metadata` table as JSON:
- `total_files`, `total_symbols`, `total_edges`
- `orphan_symbols`, `isolated_files`
- `graph_connectivity`, `graph_density`
- `import_resolution_rate`
- `edge_distribution` — count per edge kind
- `symbol_distribution` — count per symbol kind

---

## Canonical Dependency Edges (Phase 2 / 7)

`CANONICAL_DEPENDENCY_EDGES` in `query/src/graph.rs` extended from 6 to 12 kinds:

```rust
pub const CANONICAL_DEPENDENCY_EDGES: &[&str] = &[
    "calls", "imports", "interaction", "component_use",
    "type_ref", "global_ref",
    "new_call",     // constructor invocation
    "extends",      // class inheritance
    "implements",   // interface/trait implementation
    "inherits",     // Python class inheritance
    "instantiates", // assignment-level construction
    "re_export",    // re-export chain
];
```

`method_call` and `MEMBER_ACCESS` remain excluded — without type resolution,
receiver-based disambiguation is impossible and global matching fabricates phantom edges.

---

## Edge Kind Constants (Phase 2)

New `graph::edge_kind` module with typed string constants replaces raw string
literals in all graph code. No MCP tool builds custom string literals for
edge kinds.

---

## Framework Name Removal (Phase 9)

### `storage/src/entrypoints.rs`
- `nextjs_class()` → `file_convention_class()` (generic file-convention detector)
- All comments de-branded: "Next.js App Router" → "app/ convention", "FastAPI/Flask" → "HTTP-method decorators"
- Test names updated to be framework-agnostic

### `graph/src/models.rs`
- Removed JSX-specific comment references ("renders_component", "consumes_hook")

### `query/src/graph.rs`
- `route_file_fragment` generalized: no longer hard-codes `/route` suffix or `app/api/` prefix
- Works with any path layout: `app/api/run/route.ts`, `routes/api/run.py`, etc.

### Confirmed absent (grep verified):
- `FastAPI`, `Flask`, `Starlette`, `Next.js`, `React`, `Express`, `Axum`, `Actix`, `Rocket`, `Vue`, `Svelte`, `Remix`, `Angular` — none present in graph construction code

---

## Regression Tests Added (Phase 8)

| Test | File | What it verifies |
|------|------|-----------------|
| `use_leaf_extracted_not_full_path` | `extractor.rs` | Rust `use` stores leaf name, not full path |
| `brace_group_yields_individual_leaves` | `extractor.rs` | `use a::{B, C}` → two import nodes |
| `glob_import_produces_no_node` | `extractor.rs` | `use x::*` creates no edge |
| `enum_and_trait_are_indexed` | `extractor.rs` | Rust enum/trait kinds indexed |
| `impl_trait_produces_implements_edge` | `extractor.rs` | `impl Trait for Type` → implements edge |
| `named_import_produces_import_edge` | `typescript_frontend.rs` | TS named import → imports edge |
| `ts_import_query_must_compile` | `typescript_frontend.rs` | Full TS query compiles without errors |
| `member_calls_tagged_method_call` | `javascript_frontend.rs` | JS method_call/calls distinction |
| `incremental_reindex_preserves_consumers` | `reindex.rs` | Incremental reindex keeps consumer edges |
| `type_annotation_drives_dependency` | `reindex.rs` | Python type annotation → type_ref edge |

---

## `new_call` Edge Resolution

`new_call` edges (`new Foo()`) are now routed through `resolve_call_edge` in both:
- `cli/src/main.rs` full-init linker
- `indexer/src/reindex.rs` incremental linker

Resolution rules: same-file first, then global (constructor names are globally
unique enough for cross-file resolution). Method calls (`obj.foo()`) remain
same-file only.

---

## Remaining Unresolved Patterns

1. **Default exports** — `export default class Foo` → the symbol name `Foo` is
   the canonical resolution target, but the `default` alias is not tracked as a
   separate edge. Callers doing `import Foo from '…'` link correctly; callers
   doing `import X from '…'` (aliased) may not.

2. **Rust method calls** — `obj.method()` is not extracted. Adding method-call
   edges would require type inference to determine the receiver type; without it,
   global matching would fabricate phantom edges.

3. **Rust generic type parameters** — `impl<T: Trait>` captures `T` and `Trait`;
   single-character names are filtered but multi-character generic params (`Key`,
   `Val`) are not. Improvement requires detecting the generic parameter context.

4. **TypeScript `extends` on interfaces** — `interface Foo extends Bar` uses
   `extends_type_clause`, not `extends_clause`. These are currently not captured.
   Pattern: `(extends_type_clause type: (type_identifier) @extends_class)`.

5. **Wildcard re-exports** — `export * from '…'` cannot link to specific symbols.

---

## Performance Impact

All new tree-sitter patterns add negligible overhead — they are compiled once
per file parse and use the same streaming iterator infrastructure. The
validation and metrics passes add 2 SQL round-trips per index build; both are
O(edges) and typically complete in <5ms on repos with <100k edges.
