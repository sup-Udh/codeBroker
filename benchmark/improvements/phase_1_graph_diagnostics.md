# Phase 1: Graph Diagnostics & Validation

## Overview
This phase introduces a completely new Graph Diagnostics layer that validates the Universal Symbol Graph immediately after indexing. It acts as an objective quality gate to measure graph correctness before MCP tools consume the data.

## Architecture
The diagnostic engine is implemented as a standalone, extensible workspace crate (`graph_diagnostics`). 
It uses a plugin-based architecture centered around the `GraphValidator` trait. New diagnostics (e.g., for Python decorators, JSX composition, etc.) can be added by implementing this trait and registering the validator with the `DiagnosticsEngine`.

## Diagnostics Modules & Collected Metrics

### Symbol Diagnostics (`SymbolValidator`)
- **Duplicate Definitions**: Detects symbols defined multiple times at the same location (typically caused by greedy or overlapping Tree-sitter captures).
- **Unnamed Symbols**: Identifies symbols indexed with an empty or null name.

### Edge Diagnostics (`EdgeValidator`)
- **Dangling Edges**: Finds edges referencing missing or deleted target/source symbols.
- **Duplicate Edges**: Highlights edges inserted multiple times.
- **Self Loops**: Detects edges where the source symbol points to itself (recursion vs. bad local attribution).

### Import Diagnostics (`ImportValidator`)
- **Unresolved Imports**: Finds `relationships` (kind `imports`) that never produced an edge.
- The validator classifies failures into actionable likely causes: Alias failures, External dependencies, Missing files, or Namespace imports.

### Call Diagnostics (`CallValidator`)
- **Unresolved Calls**: Identifies `relationships` (kinds `calls`, `method_call`, `new_call`) missing resolution edges.
- Classifies missing method calls as dynamic/type-inference dependent, and missing free calls as unresolved globals.

## Graph Health Computation
The overall graph health is not hardcoded but derived flexibly from metrics:
1. Base score derived from **Import Resolution Rate**.
2. Penalties applied for structural anomalies: **Dangling Edges** (up to 10% penalty) and **Duplicate Edges** (up to 5% penalty).

A final PASS/FAIL gate ensures the graph cannot pass if any validator yields a `CRITICAL` or `ERROR` severity finding.

## Future Extension Points
The plugin architecture easily accommodates future validators:
- **JSX Composition**
- **Python Decorators**
- **Rust Traits**
- **Macro Expansion**
- **Generic Type Resolution**
- **Interaction Edges**

These can be added by implementing `GraphValidator` without modifying the core engine.
