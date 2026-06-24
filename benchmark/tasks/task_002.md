# task_002.md

# Real Feature Implementation Workflow

Objective:

Implement a medium-complexity feature in the repository.

The goal is to test whether CodeBroker provides enough discovery and context to safely modify production code.

Before making any changes:

1. Discover affected subsystems.
2. Locate all relevant symbols.
3. Identify implementation entrypoints.
4. Trace dependencies.
5. Determine blast radius.
6. Gather edit context.
7. Produce an implementation plan.

Mandatory CodeBroker Tools:

* generate_context_capsule
* search_codebase
* find_symbol
* get_context
* get_implementation
* get_edit_context
* impact_analysis
* explore_graph
* graph_subtree
* read_symbol_source
* read_file_skeleton

Optional:

* generate_patch

Evaluation Questions:

* Did CodeBroker find the correct files?
* Was edit context sufficient?
* Was dependency tracing accurate?
* Was blast radius analysis useful?
* Did CodeBroker reduce repository exploration?
* Were any native discovery tools required?

Generate a benchmark report after implementation.
