with open('cli/src/main.rs', 'r') as f:
    content = f.read()

start_marker = "let t_pass2 = std::time::Instant::now();"
end_marker = "t_edge_insert_total as f64 / total_relationships.max(1) as f64,\n                );\n"

start_idx = content.find(start_marker)
end_idx = content.find(end_marker)

if start_idx != -1 and end_idx != -1:
    end_idx += len(end_marker)
    
    replacement = """let t_pass2 = std::time::Instant::now();
                let (edges_created, total_relationships) = indexer::linker::resolve_relationships(&db, None).expect("Linker failed");
                println!("Linking complete. Created {} true graph edges from {} relationships.", edges_created, total_relationships);
                eprintln!("[TIMING] Pass 2 (edge linking): {}ms", t_pass2.elapsed().as_millis());
"""
    new_content = content[:start_idx] + replacement + content[end_idx:]
    
    # Remove resolve_call_edge
    rc_start = new_content.find("fn resolve_call_edge(")
    rc_end = new_content.find("// 1. Define the CLI arguments")
    if rc_start != -1 and rc_end != -1:
        # Check if there are comments before it that we should remove too
        # Actually it's fine to just remove from rc_start
        # Let's find the `///` doc comment before it
        doc_start = new_content.rfind("/// Resolves a call", 0, rc_start)
        if doc_start != -1:
            rc_start = doc_start
        new_content = new_content[:rc_start] + new_content[rc_end:]

    with open('cli/src/main.rs', 'w') as f:
        f.write(new_content)
    print("Patched cli/src/main.rs")
