import re

files_to_fix = [
    "query/src/context.rs",
    "query/src/engine.rs",
    "query/src/graph.rs"
]

for path in files_to_fix:
    with open(path, "r") as f:
        content = f.read()

    # Remove prop_type: None,
    content = re.sub(r'\s*prop_type:\s*None,?', '', content)

    with open(path, "w") as f:
        f.write(content)

