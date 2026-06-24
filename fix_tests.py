import os

def fix_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()
    
    # Simple replacement: replace `signature: None,` with `signature: None, route_path: None, route_method: None,` ONLY IF it's not already there.
    # We will just replace `signature: None` where it does NOT have route_path.
    import re
    
    def repl(m):
        return 'signature: None, route_path: None, route_method: None'
        
    content = re.sub(r'signature:\s*None(?!, route_path:)', repl, content)
    
    # Some might have `signature: None\n` or similar. Let's be careful.
    content = re.sub(r'signature:\s*None,\s*\}', r'signature: None, route_path: None, route_method: None, }', content)
    content = re.sub(r'signature:\s*None\s*\}', r'signature: None, route_path: None, route_method: None }', content)
    
    # Actually, a better way is to just replace all `signature: None` with `signature: None, route_path: None, route_method: None` then remove duplicates.
    content = content.replace('signature: None', 'signature: None, route_path: None, route_method: None')
    content = content.replace('route_path: None, route_method: None, route_path: None, route_method: None', 'route_path: None, route_method: None')
    content = content.replace('route_path: None, route_method: None, route_method: None', 'route_path: None, route_method: None')
    content = content.replace(', route_path: None, route_path: None', ', route_path: None')
    content = content.replace(', route_method: None, route_method: None', ', route_method: None')

    with open(filepath, 'w') as f:
        f.write(content)

for f in ['query/src/context.rs', 'query/src/duplicates.rs', 'query/src/engine.rs', 'cli/src/main.rs', 'semantic/src/embeddings.rs']:
    fix_file(f)

