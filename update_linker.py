import re

# Update cli/src/main.rs
with open('cli/src/main.rs', 'r') as f:
    content = f.read()

# Replace Pass 2 block
# We know it starts with "let t_pass2 = std::time::Instant::now();"
# and ends with "println!("Linking complete. Created {} true graph edges.", edges_created);"
pass2_start = content.find('let t_pass2 = std::time::Instant::now();')
pass2_end = content.find('println!(\"Linking complete. Created {} true graph edges.\",', pass2_start)

if pass2_start != -1 and pass2_end != -1:
    # Find the end of the println block
    end_of_println = content.find(';', pass2_end) + 1
    
    # We will replace everything from t_pass2 to end_of_println
    # with the call to our new linker
    
    replacement = """let t_pass2 = std::time::Instant::now();
                
                let (edges_created, total_relationships) = indexer::linker::resolve_relationships(&db, None).expect("Linker failed");
                
                println!("Linking complete. Created {} true graph edges from {} relationships.", edges_created, total_relationships);
"""
    new_content = content[:pass2_start] + replacement + content[end_of_println:]
    
    # Also remove `fn resolve_call_edge` and `fn build_alias_map` if it's there
    # since they are now in linker.rs. Wait, `resolve_call_edge` is at the top of cli/main.rs.
    # We can just leave it or remove it. It's safer to remove it so we don't have dead code.
    resolve_call_start = new_content.find('fn resolve_call_edge(')
    if resolve_call_start != -1:
        # Find its end (just before `// 1. Define the CLI arguments`)
        cli_args_start = new_content.find('// 1. Define the CLI arguments')
        if cli_args_start != -1:
            new_content = new_content[:resolve_call_start] + new_content[cli_args_start:]
    
    with open('cli/src/main.rs', 'w') as f:
        f.write(new_content)
    print("Updated cli/src/main.rs")

# Update indexer/src/reindex.rs
with open('indexer/src/reindex.rs', 'r') as f:
    content = f.read()

reindex_start = content.find('let relationships = db')
reindex_end = content.find('// 3. Infer logical interactions')

if reindex_start != -1 and reindex_end != -1:
    replacement = """let (edges_created, _) = crate::linker::resolve_relationships(db, Some(&touched_file_ids)).unwrap_or((0,0));
    stats.edges_created += edges_created;
    
    """
    new_content = content[:reindex_start] + replacement + content[reindex_end:]
    
    with open('indexer/src/reindex.rs', 'w') as f:
        f.write(new_content)
    print("Updated indexer/src/reindex.rs")

