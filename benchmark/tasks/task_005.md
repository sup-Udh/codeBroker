# task_005.md

# Discovery Stress Test

Objective:

Evaluate CodeBroker's repository discovery capabilities without relying on filenames or explicit symbol names.

You are NOT allowed to manually browse the repository before discovery.

Discovery Challenges:

1. Find the authentication system.
2. Find the real-time collaboration system.
3. Find room management functionality.
4. Find notification infrastructure.
5. Find repository entrypoints.
6. Find the most critical business logic.
7. Find shared infrastructure used by multiple subsystems.

Mandatory CodeBroker Tools:

* generate_context_capsule
* search_codebase
* find_symbol
* subsystem_stats
* subsystem_overview
* project_overview

Evaluation Questions:

* Did discovery surface the correct locations?
* Were search results relevant?
* Did CodeBroker return misleading matches?
* Did natural language discovery work?
* Was manual repository exploration required?

Generate a benchmark report when complete.
