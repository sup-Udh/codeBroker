import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

log_ctx_code = """
        // Gather adjacent symbols for relevance scoring
        eprintln!("Gathering context for name: {}, hint: {}", name, rel_hint);
        if let Ok(Some(ctx)) = query::context::ContextObject::assemble_scoped(db, name, Some(rel_hint)) {
            eprintln!("Found ctx for {} in {}. fwd: {}, rev: {}, callees: {}, callers: {}", name, ctx.defining_file, ctx.forward_dependencies.len(), ctx.reverse_dependencies.len(), ctx.callees.len(), ctx.callers.len());
"""

content = content.replace("""        // Gather adjacent symbols for relevance scoring
        if let Ok(Some(ctx)) = query::context::ContextObject::assemble_scoped(db, name, Some(rel_hint)) {""", log_ctx_code)

with open('mcp/src/main.rs', 'w') as f:
    f.write(content)
