use query::context::ContextObject;

pub fn build_prompt(
    symbol_name: &str,
    source_code: &str,
    context: &ContextObject,
    config_files_text: &str,
) -> String {
    let json_context = serde_json::to_string_pretty(context).unwrap_or_default();

    // We construct a massive, highly structured string using the format! macro
    format!(
        "You are an expert AI software architect. Analyze the following code symbol and explain its purpose, its role in the system, and what impact modifying it would have based on its blast radius.

Target Symbol: {symbol_name}

=== SYSTEM CONFIGURATION (Context) ===
The following are critical configuration files in this repository (e.g. package.json, Dockerfile) that tell you what technologies and infrastructure this project uses:
{config_files_text}

=== GRAPH DEPENDENCIES (Blast Radius) ===
The following JSON object contains the exact dependencies, reverse dependencies, and sibling symbols for the target symbol:
```json
{json_context}
```

=== SOURCE CODE ===
```
{source_code}
```

Keep your summary concise, highly technical, and focus on system architecture."
    )
}
