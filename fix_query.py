import os
import re

def fix_retrieval():
    path = "query/src/retrieval.rs"
    with open(path, "r") as f:
        content = f.read()

    content = content.replace("pub route_path: Option<String>,", "pub attributes: Vec<String>,")
    content = content.replace("pub route_segment: Option<String>,", "pub metadata: Option<String>,")
    
    # DB queries
    content = content.replace("files.directive, files.route_path, files.route_segment", "symbols.attributes, symbols.metadata")
    
    # Row gets
    content = content.replace("let route_path: Option<String> = row.get(7).unwrap_or(None);", "let attributes_str: Option<String> = row.get(7).unwrap_or(None);\n        let attributes = attributes_str.map(|s| serde_json::from_str(&s).unwrap_or_default()).unwrap_or_default();")
    content = content.replace("let route_segment: Option<String> = row.get(8).unwrap_or(None);", "let metadata: Option<String> = row.get(8).unwrap_or(None);")
    
    # Struct instantiation
    content = content.replace("route_path,\n            route_segment,", "attributes,\n            metadata,")

    with open(path, "w") as f:
        f.write(content)

fix_retrieval()
