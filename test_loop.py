import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

log_code = """
            for (cand, rel_type) in candidates {
                eprintln!("Checking candidate: {}", cand);
                if let Ok(cand_sources) = query::retrieval::read_symbol_source_scoped(db, &cand, true, None) {
                    eprintln!("Got {} sources for candidate: {}", cand_sources.len(), cand);
                    for cand_src in cand_sources {
                        eprintln!("Source for {}: {}", cand, cand_src.file_path);"""

content = content.replace("""            for (cand, rel_type) in candidates {
                if let Ok(cand_sources) = query::retrieval::read_symbol_source_scoped(db, &cand, true, None) {
                    for cand_src in cand_sources {""", log_code)

with open('mcp/src/main.rs', 'w') as f:
    f.write(content)
