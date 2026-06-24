# task_004.md

# Refactoring & Duplicate Logic Audit

Objective:

Evaluate CodeBroker's ability to discover duplication, estimate refactor risk, and prepare safe code changes.

Requirements:

1. Identify duplicate logic.
2. Identify near-duplicate logic.
3. Determine what can be shared.
4. Determine what should remain separate.
5. Estimate blast radius of consolidation.
6. Identify affected files and symbols.
7. Generate a refactor plan.
8. Generate proposed patches.

Mandatory CodeBroker Tools:

* find_duplicate_logic
* impact_analysis
* get_edit_context
* get_context
* get_implementation
* read_symbol_source
* generate_patch

Evaluation Questions:

* Did duplicate detection find useful results?
* Were false positives generated?
* Was impact analysis accurate?
* Were generated patches useful?
* Did CodeBroker provide enough context for refactoring?

Generate a benchmark report when complete.


