use tree_sitter::{Node, Parser};

pub fn normalize_snippet(source_code: &str, extension: &str) -> Option<(String, usize)> {
    let language = match extension {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "js" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        _ => return None,
    };

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source_code, None)?;

    let mut output = String::new();
    let count = walk_and_normalize(tree.root_node(), source_code.as_bytes(), &mut output);

    // Trivial boilerplate filters (e.g. standard logger init or empty classes)
    if output.contains("logger") || output.contains("get_logger") {
        return None;
    }

    Some((output, count))
}

fn walk_and_normalize(node: Node, source: &[u8], output: &mut String) -> usize {
    if !node.is_named() {
        return 0; // Skip syntax tokens like {, }, (, ), ;, ,
    }

    let kind = node.kind();
    let mut node_count = 1;

    // Normalize literals and identifiers
    if kind == "identifier"
        || kind == "type_identifier"
        || kind == "property_identifier"
        || kind == "shorthand_property_identifier"
    {
        output.push_str("#ID ");
    } else if kind == "string" || kind == "string_literal" || kind == "template_string" {
        output.push_str("#STR ");
    } else if kind == "number" || kind == "integer" || kind == "float" {
        output.push_str("#NUM ");
    } else if kind == "comment" || kind == "line_comment" || kind == "block_comment" {
        // Ignore comments entirely
    } else {
        output.push_str(kind);
        output.push(' ');

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            node_count += walk_and_normalize(child, source, output);
        }
    }

    node_count
}
