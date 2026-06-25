# CodeBroker Fix List — Graph & Retrieval Regression Findings

Generated from a manual regression pass against the OrcaAI workspace
(`/home/labuser/code/netwin`), exercising `get_context`, `explore_graph`,
`impact_analysis`, `shortest_path`, `find_duplicate_logic`, `subsystem_stats`,
and `search_codebase` on the `simulate` symbol (`orchestrator/main.py:206`)
and related subsystems. Each item below has: the bug, why it's a bug
(expected vs actual, verified against ground truth), the root cause in the
codebase, and the concrete fix to implement.

Work through items in order — they are roughly ordered by impact. After each
fix, re-run the verification steps listed so regressions are caught before
moving to the next item.

---

## 1. `fetch_siblings` returns local variables instead of true module-level siblings

**File:** `query/src/context.rs:187-199` (`fetch_siblings`)

**Bug:** The query is:

```sql
SELECT name FROM symbols WHERE file_id = ?1 AND name != ?2
```

This selects *every* symbol row with the same `file_id`, with no filter on
`kind` or scope. Local variables, function parameters, and loop variables are
apparently stored as `symbols` rows tagged with the same `file_id` as their
enclosing function, so they leak into the sibling set.

**Verified impact:** Calling `get_context`/`impact_analysis` on `simulate` in
`orchestrator/main.py` returned ~70 "siblings", of which ~95% were local
variables assigned *inside* `simulate` itself (`container_name`,
`raw_traffic`, `node_container_map`, `network_name`, `simulation_id`, …) plus
locals from *other* unrelated functions in the same file (`engine`, `event`,
`weights`, `anonymous_pct`). Cross-checked against `read_file_skeleton` for
that file: the true top-level siblings are exactly 6 Pydantic model classes,
8 route functions, and a handful of module-level constants (`IMAGE_MAP`,
`METRICS_QUEUES`, `FAILURE_ENGINES`, `BOTTLENECK_REGISTRY`, `app`, `client`,
`logger`) — none of the local-variable noise belongs there.

**Fix:**
- Add a `kind` filter to the query so only symbol kinds that represent
  real top-level declarations are returned (e.g. `function`, `method`,
  `class`, `interface`, `const`/module-level `variable` — exclude any kind
  used for locals/params, e.g. `local_variable`/`parameter` if those are
  distinct kinds in the schema; if locals share the same `kind` value as
  module constants, add a `scope`/`parent_symbol_id IS NULL` condition
  instead so only symbols with no enclosing function/method are returned).
- Concretely: check the `symbols` table schema for a column that
  distinguishes top-level declarations from nested locals (likely
  `parent_symbol_id` or similar). The fix should look like:
  ```sql
  SELECT name FROM symbols
  WHERE file_id = ?1 AND name != ?2 AND parent_symbol_id IS NULL
  ```
  Adjust the column name to whatever the indexer actually populates — check
  `indexer/src` for how local variables are inserted into `symbols` to find
  the right discriminator.

**Verify:** Re-run `get_context("simulate", include_source=false)` and
`impact_analysis("simulate", format="structured")` against OrcaAI; siblings
should be limited to `Config, Node, Edge, LoadProfile, PersonaDef, Topology,
root, analyze_topology, supported_software, get_simulation_failures,
get_simulation_bottlenecks, simulation_events, simulation_metrics` (i.e. no
`container_name`, `raw_traffic`, etc.)

---

## 2. `explore_graph` does not filter edge kinds, so outgoing/incoming traversal is polluted with variable-read edges

**File:** `query/src/graph.rs:234-244` (`out_stmt` / `in_stmt` inside
`explore_graph`)

**Bug:** The traversal queries are:

```sql
SELECT target_symbol_id, kind, edge_type FROM edges WHERE source_symbol_id = ?1
SELECT source_symbol_id, kind, edge_type FROM edges WHERE target_symbol_id = ?1 AND source_symbol_id IS NOT NULL
```

Neither query filters by edge `kind`. Compare this to
`query/src/context.rs:201-204` (`fetch_forward_dependencies`), which calls
`crate::graph::get_outgoing_edges(self.db, id, Some("imports"))` — i.e.
elsewhere in the codebase, edge-kind filtering is the norm, but
`explore_graph` skips it.

**Verified impact:** `get_context("simulate")` and `impact_analysis("simulate")`
both correctly report 7 real callees (`FailureEngine, compile_topology,
execute_traffic, find_path, generate_traffic, run_simulation,
stream_metrics`). `explore_graph("simulate", direction="outgoing")` returns
those same 7 *plus* ~25 extra nodes that are local variables
(`simulation_id`, `path`, `status`, `container`, `network_graph`, …)
connected via `MEMBER_ACCESS`/`instantiates` edges. The three tools should
present a consistent dependency picture for the same symbol; right now they
don't.

**Fix:**
- Decide on a canonical set of "real dependency" edge kinds (likely `calls`,
  `imports`, `extends`/`implements`) that should be the default for
  `explore_graph`.
- Either:
  (a) Add an edge-kind filter to `out_stmt`/`in_stmt` by default (e.g.
  `WHERE source_symbol_id = ?1 AND kind IN ('calls','imports','instantiates','extends')`,
  excluding `MEMBER_ACCESS` and any other read/access-tracking edge kind), or
  (b) Add a new optional parameter to the `explore_graph` MCP tool (e.g.
  `edge_kinds`) defaulting to the curated set above, while still allowing
  power users to opt into the full unfiltered graph if needed.
  Prefer (a) — the MCP tool description promises "dependency graph"
  traversal, and `MEMBER_ACCESS` edges to plain variables are not
  dependencies in that sense.
- This also indirectly fixes the `instantiates`-on-a-variable issue in
  item 3 below if the variable nodes are removed from traversal entirely.

**Verify:** Re-run `explore_graph("simulate", direction="outgoing", depth=3)`;
the `nodes`/`edges` result should match the 7 callees from `get_context`
(plus whatever those callees themselves call, since depth=3), with no
`variable`-kind nodes appearing.

---

## 3. Duplicate/mistyped edges between the same node pair

**File:** edge-construction logic feeding the `edges` table — search
`indexer/src` and `graph/src` for where `kind` values `"calls"`,
`"imports"`, and `"instantiates"` are assigned (likely separate passes for
call-resolution vs. import-resolution vs. type-inference).

**Bug:** Observed in `explore_graph` output for `stream_metrics`:
- `stream_metrics → detect_bottlenecks` appears twice with two different
  `kind` values: `"imports"` and `"calls"`. A single relationship between
  two functions shouldn't be recorded under two different edge kinds when
  one of them is just wrong (a function `import`s a module-level function
  it calls, but the call itself should be `"calls"`, not also a duplicate
  `"imports"` edge to the same target symbol).
- `stream_metrics → bottlenecks` (a plain list/variable, not a class) is
  tagged `"instantiates"`. `instantiates` should never have a non-class
  target.

**Fix:**
- Locate the indexer pass(es) that emit `"imports"` edges and confirm
  whether they're meant to point at symbols (functions/classes) imported
  into a file, vs. calls made to those symbols. If a function is imported
  *and* called, that's legitimately two semantically different facts, but
  they shouldn't collide into "the same edge with two kinds" in a way that
  surfaces as redundant noise in traversal — consider whether `"imports"`
  edges should be scoped to file-level (no `source_symbol_id`) rather than
  attributed to whichever function happens to call the imported symbol.
- Add a validation/assertion at edge-insert time (or a post-index sanity
  pass) that `instantiates` edges only target symbols with
  `kind = 'class'`. Anything else indicates a resolution bug in whichever
  pass infers `instantiates` (likely conflating attribute/member access on a
  variable with object construction) — fix the resolver to only emit
  `instantiates` when the target is a known class definition.

**Verify:** Re-run `shortest_path("simulate", "detect_bottlenecks")`; the
edge `stream_metrics → detect_bottlenecks` should report a single,
correctly-typed `kind` (`"calls"`), and no edge `kind="instantiates"` should
target a `variable`-kind symbol anywhere in the OrcaAI workspace graph.

---

## 4. `subsystem_stats` includes files with zero relation to the queried subsystem

**File:** `query/src/subsystem.rs:181-280` (`discover_subsystem`, "Seed
Generation" + "Graph-Based Expansion" sections)

**Bug:** `discover_subsystem` works in two phases:
1. **Seed generation** (`subsystem.rs:188-199`) runs `search_symbols` (the
   same fuzzy/semantic search backing `search_codebase`) using the
   subsystem name as the query, and admits any result with
   `score >= 100 || confidence starts_with "High"/"Medium"` (line 210) —
   this is a low bar that lets semantically-adjacent-but-unrelated symbols
   into the seed set (e.g. `authenticated_pct`/`anon_pct`/`auth_pct`
   traffic-mix fields for a query of `"auth"`).
2. **Graph-based expansion** (`subsystem.rs:242-280`) then runs **2 rounds**
   of unscoped BFS over *all* incoming/outgoing edges from every seed symbol
   ("Route Ownership Expansion" and "Shared Dependency Expansion"), with no
   relevance check against the original subsystem name — any symbol that
   merely shares a dependency with a seed gets pulled in.

   The combination of a loose seed + 2 unscoped expansion hops is how
   `subsystem_stats("db")` ends up including
   `OrcaAI/frontend/package.json` (confirmed by direct inspection of that
   file's contents — it contains no occurrence of the string `"db"`
   anywhere) and how `subsystem_stats("auth")` pulls in
   `simulation_engine.py`, `traffic_executor.py`, `session_engine.py`, and
   `metrics_engine.py`, none of which implement authentication.

**Verified impact:** `subsystem_stats("db")` returned files
`["containers/db/app.py", "frontend/package.json",
"import_engine/file_parsers.py", "import_engine/topology_mapper.py",
"main.py", "metrics_engine.py", "request_types.py", "session_engine.py",
"software_registry_v2.py", "user_model.py", "user_personas.py"]` with
`"confidence": "High"` — i.e. it confidently reports a wrong subsystem
boundary, not just a noisy one.

**Fix:**
- Tighten the seed-admission threshold at `subsystem.rs:210`. At minimum,
  require an exact or near-exact name/path match (not a fuzzy/semantic
  "Medium" confidence hit) before a symbol is allowed to seed subsystem
  discovery — semantic search is good for `search_codebase` (where the
  *query itself* is meant to be fuzzy) but `subsystem_stats` takes a
  specific folder/module name as input and should bias toward precision.
- Cap or remove the "Shared Dependency Expansion" round (`subsystem.rs`
  section B, ~line 265-280) — pulling in *any* symbol that merely shares a
  callee with a seed symbol is unbounded topic drift, especially across 2
  rounds. Consider requiring a minimum shared-dependency count (e.g. ≥3
  shared edges) or dropping this expansion entirely in favor of only "Route
  Ownership Expansion" (which is more conservative since it requires the
  caller to look like a real entrypoint, `score >= 50`).
  this expansion pass loses, so it can be deprioritized below the other two)
- Add a final filter: after expansion, drop any file/symbol whose path or
  name has no token overlap with the subsystem `name` or with any seed
  symbol that itself directly matched (not just transitively reached) the
  name.

**Verify:** Re-run `subsystem_stats("db")` and `subsystem_stats("auth")`
against OrcaAI. `"db"` should return only `containers/db/app.py` and files
that directly reference DB connections/queries (e.g. wherever
`DB_QUERY`/`USERS` are genuinely used for a database, not just defined as an
unrelated enum value) — it must **not** include
`frontend/package.json`. `"auth"` should return only
`containers/auth/app.py`, `containers/web/app.py` (since it calls
`/auth/validate`), and `main.py` (the FastAPI routes) — not
`simulation_engine.py`, `traffic_executor.py`, `session_engine.py`, or
`metrics_engine.py`.

---

## 5. `find_duplicate_logic` has a high false-positive rate on short statements

**File:** `query/src/duplicates.rs:39-112` (`find_duplicate_logic`)

**Bug:** Duplicate detection groups symbols by `hash(normalize(source))`
after filtering on `normalized.len() < min_normalized_len` (default 80
chars, `duplicates.rs:85`). For single-line variable assignments, the
AST-normalized form of unrelated statements collapses to the same short
string/length purely because they have the same shallow shape (e.g.
`client = docker.from_env()`, `stop_event = asyncio.Event()`,
`p_strip = ...`, `current = ...` all landed in one "duplicate" group despite
having no semantic relationship). Only ~2 of the 13 groups returned for the
OrcaAI `orchestrator` subsystem were meaningful (13x repeated
`logger = logging.getLogger(...)` boilerplate, and structurally similar
small Pydantic model classes) — the rest were one-liner noise.

**Fix:**
- Raise the practical minimum block size for single-statement bodies — e.g.
  require either (a) a minimum *statement count* (≥3) in addition to/instead
  of a raw character-length threshold, so trivial one-line
  assignments/instantiations never qualify regardless of how the
  normalization happens to size them, or (b) weight the hash by AST shape
  *and* identifier/literal diversity so that two statements with totally
  different RHS expressions don't collapse to one hash bucket just because
  they normalize to the same token count.
- Default `min_length` (80) is reachable by single statements after
  normalization — consider raising the CLI/MCP default (e.g. 150-200) as an
  interim mitigation while the statement-count fix lands, and document the
  trade-off (lower default = more recall on tiny duplicated snippets, more
  false positives on boilerplate one-liners).

**Verify:** Re-run `find_duplicate_logic(path_scope="orchestrator")` on
OrcaAI; the `logger = logging.getLogger(...)` and small-Pydantic-class
groups should remain, but groups like `{client, stop_event, start_time,
current, p_strip}` (unrelated one-line variable inits bundled only by
char-length) should disappear.

---

## Problems encountered while using CodeBroker during this evaluation (for awareness, not necessarily code fixes)

These aren't separate bugs from the 5 above, but document friction
encountered while running the regression pass itself, in case they point to
additional polish items:

- **`get_context` silently omits keys instead of returning empty arrays.**
  For `simulate` (a route entrypoint with no callers), the response had no
  `callers`, `dependencies`, or `siblings` keys at all rather than
  `"callers": []` etc. This is harmless once you know about it, but it's
  easy to mistake "key omitted" for "data unavailable / tool error" versus
  "the value is genuinely empty." Consider always emitting the full key set
  with empty-array defaults for consistency with documented behavior
  ("Returns deterministic graph context for a symbol, including callers,
  callees, dependencies, siblings, and related symbols").
- **No way to ask `explore_graph`/`impact_analysis` to restrict to a
  specific edge-kind set from the MCP interface** — this evaluation had to
  infer the discrepancy between tools by manually diffing their outputs;
  once item 2 above is fixed, it'd be useful to expose the edge-kind filter
  as an optional parameter so future regression passes can verify it
  directly instead of inferring it from raw edge dumps.
- **`subsystem_stats` returns `"confidence": "High"` even when the result
  set is wrong** (see item 4) — confidence is computed from the seed match
  quality, not validated against the final expanded file set. Once item 4's
  expansion-pruning fix lands, consider also down-grading reported
  confidence when the expansion phase pulls in files with low/no token
  overlap with the subsystem name, so a wrong-but-still-occurring result is
  at least flagged as lower confidence rather than "High."
- **`search_codebase` natural-language results are accurate at the top but
  noisy by rank 10-15** — not a correctness bug (top-ranked results were
  relevant in both NL queries tested), but worth knowing if an agent is
  consuming the full result list rather than just the top few: results
  past the top 5-10 are dominated by single unrelated variables and
  `package.json` dependency/script entries ranked by raw semantic-similarity
  score with no precision cutoff. A `min_confidence` filter exists as a
  parameter already — consider defaulting tool-facing summaries to a
  stricter cutoff (e.g. `Medium` and above) rather than returning ~40
  results of mixed quality by default.

---

## Suggested implementation order

1. Item 1 (`fetch_siblings`) — smallest, most self-contained fix, highest
   correctness impact on every `get_context`/`impact_analysis` call.
2. Item 2 (`explore_graph` edge filtering) — also self-contained, fixes the
   cross-tool disagreement directly.
3. Item 4 (`subsystem_stats` expansion) — most impactful "produces a wrong
   answer with high confidence" bug; takes more care since it involves
   tuning thresholds, so do it after the two simpler graph fixes.
4. Item 3 (duplicate/mistyped edges) — likely requires touching indexer
   resolution passes; do after the query-layer fixes since it's lower
   blast-radius (mostly noise, not wrong answers) and may require
   re-indexing test fixtures to verify.
5. Item 5 (`find_duplicate_logic` thresholds) — independent of the rest,
   lowest urgency since it's a quality/precision tuning issue rather than a
   correctness bug.

For each item, re-run the "Verify" steps against the OrcaAI workspace
(`/home/labuser/code/netwin`, root `OrcaAI/`) before moving to the next.
