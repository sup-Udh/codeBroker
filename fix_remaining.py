import re

files_to_fix = [
    "semantic/src/embeddings.rs",
    "cli/src/main.rs"
]

for path in files_to_fix:
    with open(path, "r") as f:
        content = f.read()

    # Remove prop_type: None,
    content = re.sub(r'\s*prop_type:\s*None,?', '', content)

    # Replace route_path and route_method instantiations
    content = re.sub(r'route_path:\s*None,', 'attributes: Vec::new(),', content)
    content = re.sub(r'route_method:\s*None,?', 'metadata: None,', content)
    content = content.replace("attributes: Vec::new(), metadata: None,", "attributes: Vec::new(), metadata: None,")

    with open(path, "w") as f:
        f.write(content)

