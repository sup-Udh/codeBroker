import sys

with open('mcp/src/main.rs', 'r') as f:
    content = f.read()

import re

# find the generate_context_capsule function
start_idx = content.find("fn generate_context_capsule(")
end_idx = content.find("fn add_response_size_hint(")

if start_idx == -1 or end_idx == -1:
    print("Could not find start or end index.")
    sys.exit(1)

with open('scratch_generate_context.rs', 'r') as f:
    scratch_content = f.read()

# fix the compile error in scratch
scratch_content = scratch_content.replace(".next().is_some()", ".is_empty() == false")

new_content = content[:start_idx] + scratch_content + "\n" + content[end_idx:]

with open('mcp/src/main.rs', 'w') as f:
    f.write(new_content)

print("Replacement complete.")
