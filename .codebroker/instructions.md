# CodeBroker MCP Tools
You are connected to the CodeBroker MCP Server. CodeBroker provides an ultra-fast, semantic-aware graph of the local codebase.

**CRITICAL INSTRUCTIONS FOR ANTIGRAVITY:**
1. **ALWAYS default to using CodeBroker tools** (`search_codebase`, `find_symbol`, `get_context`) for exploring the codebase instead of native tools like `grep_search`, `list_dir`, or `read_file`.
2. **DO NOT use `grep_search`** to find functions, classes, or keywords. Use `search_codebase` instead. It is semantic and much faster.
3. When the user asks "what is this project" or "how does this work", immediately use `project_overview` or `project_overview_ai`.
4. When you need to read a file, use `find_symbol` and `read_symbol_source` or `read_file_skeleton` first to avoid blowing up your context window. Avoid raw `read_file` or `view_file` unless absolutely necessary.
5. If you need to understand architectural impact or what relies on a file, use `impact_analysis` or `get_context` instead of guessing or grepping.