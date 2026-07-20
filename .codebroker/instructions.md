## CodeBroker - Repository Intelligence Layer

### MANDATORY: use CodeBroker - do NOT grep or glob the codebase blindly
For any feature, fix, or refactor request:
**call `search_codebase` or `repository_stats` FIRST**. CodeBroker is the discovery, architecture, and context engine for this workspace.

Do NOT use grep, glob, or Bash sweeps to explore the codebase. CodeBroker answers "where is it, what touches it, what breaks" cheaply and fast. Use native tools (grep/Read) ONLY for verification or when CodeBroker identifies a specific file.

### Primary Tools (Discovery & Context)
- `search_codebase` - **USE THIS TO FIND CODE**. Fuses keyword and semantic rankings. Mode: "both" works for concrete identifiers and conceptual phrases. Exact symbol matches rank first.
- `get_context` - View callers/callees at a glance. Default to `format: "markdown"`.
- `get_edit_context` - **USE BEFORE EDITING**. Returns exact line boundaries, callers, callees, and reverse dependencies in one call.
- `repository_stats` - Run once at session-start for unfamiliar repos. Returns files, languages, and entrypoints.

### Deep Dive Tools (Use only when necessary)
- `read_file_skeleton` - Preferred over full file reads to grasp structure. Merge with symbol reads (`target_symbol="X"`).
- `read_symbol_source` - Use for exact bodies; batch related symbols via `symbols: [...]`.
- `explore_graph` / `shortest_path` - Use to trace connections between subsystems.
- `subsystem_communication` - View directory-to-directory coupling.
- `architectural_hotspots` / `find_duplicate_logic` - Scope with `path_scope` once target area is known to avoid noise.

### Workflow
1. `search_codebase` (mode: "both") - ALWAYS FIRST to locate relevant files/symbols.
2. Need structure? `read_file_skeleton` - avoid full Reads unless editing.
3. Need dependencies? `get_context` (markdown) or `get_edit_context`.
4. Make targeted changes natively based on the discovered context.
5. Verify natively - CodeBroker confirms structure, not correctness (tests/compilation).

### Trust Calibration & Golden Rules
- **Dynamic Blind Spots**: The index models static imports. Decorators (Python), dynamic `import()` (JS/TS), and macros (Rust) are invisible. Verify these natively.
- **Verify generic edges**: Edge resolution is name-based. Confirm actual import/use paths for generic names (e.g. `obj.metadata`).
- **Resolve before query**: Never pass guessed paths to `get_context` or `read_symbol_source`. Confirm via `search_codebase` first.
- **High Trust**: `repository_stats`, exact symbol matches, same-subsystem graph edges.
- **Low Trust**: Cross-file edges on generic names, duplicate logic groups (might be intentional), dynamic dispatch.

### Anti-patterns
- Acting on an impact claim in auth/payments/data-integrity without native confirmation.
- Full-file reads when a skeleton plus one or two symbol reads would answer it.
- Recursive directory scans or blind grep sweeps to "explore" — use `repository_stats` and `search_codebase` instead.
- Retrying a failed subsystem/symbol name by guessing variants — use the `did_you_mean` / candidate list.
