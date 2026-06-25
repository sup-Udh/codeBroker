import os
import re

files_to_fix = [
    "query/src/context.rs",
    "query/src/engine.rs",
    "query/src/graph.rs",
    "semantic/src/embeddings.rs"
]

for path in files_to_fix:
    with open(path, "r") as f:
        content = f.read()

    # Replace route_path and route_method instantiations
    content = re.sub(r'route_path:\s*None,', 'attributes: Vec::new(),', content)
    content = re.sub(r'route_method:\s*None,?', 'metadata: None,', content)

    # Some might be route_path: None, route_method: None on the same line
    content = content.replace("attributes: Vec::new(), metadata: None,", "attributes: Vec::new(), metadata: None,")

    with open(path, "w") as f:
        f.write(content)

