use crate::frontend::LanguageFrontend;
use graph::{ImportNode, SymbolNode};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

pub struct PythonFrontend;

impl LanguageFrontend for PythonFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".py")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        _path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<ImportNode>,
    )> {
        let language = tree_sitter_python::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(source_code, None)?;

        let symbols = extract_py_symbols(&tree, source_code, language.clone());
        let imports = extract_py_imports(&tree, source_code, language);

        Some((graph::models::FileMetadata::default(), symbols, imports))
    }
}

fn extract_py_symbols(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let query_str = "
        (class_definition name: (identifier) @name) @class
        (function_definition name: (identifier) @name) @function
        (expression_statement (assignment left: (identifier) @name)) @variable
    ";

    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut kind = String::new();
        let mut def_node = None;

        for capture in m.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            if *capture_name == "name" {
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    name = text.to_string();
                }
            } else if *capture_name == "class" {
                kind = "class".to_string();
                def_node = Some(capture.node);
            } else if *capture_name == "function" {
                kind = "function".to_string();
                def_node = Some(capture.node);
            } else if *capture_name == "variable" {
                kind = "variable".to_string();
                def_node = Some(capture.node);
            }
        }

        if let Some(node) = def_node {
            let mut signature_parts = Vec::new();
            let short_name = name.clone();
            let mut route_path = None;
            let mut route_method = None;

            // 1. Check for decorators
            if let Some(parent) = node.parent() {
                if parent.kind() == "decorated_definition" {
                    let mut wcursor = parent.walk();
                    for child in parent.children(&mut wcursor) {
                        if child.kind() == "decorator" {
                            if let Ok(text) = child.utf8_text(source_code.as_bytes()) {
                                let trimmed = text.trim();
                                signature_parts.push(format!("[{}]", trimmed));

                                if trimmed.starts_with('@') && trimmed.contains('(') {
                                    let lower = trimmed.to_lowercase();
                                    if lower.contains(".get(")
                                        || lower.contains(".post(")
                                        || lower.contains(".put(")
                                        || lower.contains(".delete(")
                                        || lower.contains(".patch(")
                                    {
                                        let method = if lower.contains(".get(") {
                                            "GET"
                                        } else if lower.contains(".post(") {
                                            "POST"
                                        } else if lower.contains(".put(") {
                                            "PUT"
                                        } else if lower.contains(".delete(") {
                                            "DELETE"
                                        } else {
                                            "PATCH"
                                        };

                                        if let Some(start) = trimmed.find('(') {
                                            if let Some(end) = trimmed.rfind(')') {
                                                let args = trimmed[start + 1..end].trim();
                                                if args.starts_with('"') || args.starts_with('\'') {
                                                    route_path =
                                                        Some(args[1..args.len() - 1].to_string());
                                                    route_method = Some(method.to_string());
                                                    kind = "route".to_string();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 2. If it's a function, check if it's a method
            if kind == "function" {
                let mut current = node.parent();
                while let Some(p) = current {
                    if p.kind() == "class_definition" {
                        if let Some(class_name_node) = p.child_by_field_name("name") {
                            if let Ok(class_name) =
                                class_name_node.utf8_text(source_code.as_bytes())
                            {
                                name = format!("{}.{}", class_name, name);
                                kind = "method".to_string();
                            }
                        }
                        break;
                    }
                    if p.kind() == "decorated_definition" {
                        current = p.parent();
                        continue;
                    }
                    current = p.parent();
                }

                // 3. Extract parameters for signature
                let mut head = format!("def {}", short_name);
                if let Some(params_node) = node.child_by_field_name("parameters") {
                    if let Ok(params_text) = params_node.utf8_text(source_code.as_bytes()) {
                        let clean_params = params_text.replace("\n", "").replace("  ", " ");
                        head.push_str(&clean_params);
                    }
                }
                if let Some(return_node) = node.child_by_field_name("return_type") {
                    if let Ok(return_text) = return_node.utf8_text(source_code.as_bytes()) {
                        head.push_str(" -> ");
                        head.push_str(return_text.trim());
                    }
                }
                signature_parts.push(head);
            } else if kind == "class" {
                let mut head = format!("class {}", short_name);
                if let Some(bases_node) = node.child_by_field_name("superclasses") {
                    if let Ok(bases_text) = bases_node.utf8_text(source_code.as_bytes()) {
                        head.push_str(bases_text.trim());
                    }
                }
                signature_parts.push(head);
            }

            let signature = if signature_parts.is_empty() {
                None
            } else {
                Some(signature_parts.join(" "))
            };

            let end_line = node.end_position().row + 1;
            symbols.push(SymbolNode {
                name,
                kind,
                prop_type: None,
                start_line: node.start_position().row + 1,
                end_line,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                signature,
                route_path,
                route_method,
            });
        }
    }

    symbols.sort_by_key(|s| s.start_byte);
    symbols.dedup_by_key(|s| s.start_byte);

    symbols
}

fn extract_py_imports(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    // Grab any individual identifier inside any import statement
    let query_str = "
        (import_statement name: (_) @import)
        (import_from_statement module_name: (_) @source name: (_) @import)
        (class_definition superclasses: (argument_list (identifier) @inherits))
        (assignment right: (call function: (identifier) @instantiates))
        (call function: (identifier) @call_name)
        (call function: (attribute attribute: (identifier) @call_name))
        (call 
            function: [(identifier) @http_fn (attribute attribute: (identifier) @http_fn)]
            arguments: (argument_list (string (string_content) @http_route))
        )
        (attribute attribute: (identifier) @member_access)
    ";

    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut import_name = String::new();
        let mut import_source = String::new();
        let mut import_kind = None;
        let mut line_number = 0;

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "import" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "source" {
                    import_source = text.trim().to_string();
                } else if *capture_kind == "inherits" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("inherits".to_string());
                } else if *capture_kind == "instantiates" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("instantiates".to_string());
                } else if *capture_kind == "call_name" {
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("calls".to_string());
                } else if *capture_kind == "member_access" {
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        line_number = node.start_position().row + 1;
                        import_kind = Some("MEMBER_ACCESS".to_string());
                    }
                } else if *capture_kind == "http_fn" {
                    let fn_name = text.trim().to_string();
                    let is_http = fn_name == "get"
                        || fn_name == "post"
                        || fn_name == "put"
                        || fn_name == "delete"
                        || fn_name == "patch"
                        || fn_name == "request";
                    if is_http {
                        if let Some(route_node) = m
                            .captures
                            .iter()
                            .find(|c| query.capture_names()[c.index as usize] == "http_route")
                        {
                            if let Ok(route) = route_node.node.utf8_text(source_code.as_bytes()) {
                                let r = route.trim().to_string();
                                if r.starts_with('/') {
                                    import_name = r.clone();
                                    import_kind = Some("HTTP_CALL".to_string());
                                    line_number = node.start_position().row + 1;
                                    let m = if fn_name == "request" {
                                        "GET"
                                    } else {
                                        &fn_name
                                    };
                                    import_source = m.to_uppercase();
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(k) = &import_kind {
            if k == "HTTP_CALL" {
                imports.push(ImportNode {
                    name: import_name.clone(),
                    source: if import_source.is_empty() {
                        None
                    } else {
                        Some(import_source.clone())
                    },
                    line_number,
                    kind: Some(k.clone()),
                });
                import_name.clear(); // prevent duplicate insertion
            }
        }
        if !import_name.is_empty() {
            imports.push(ImportNode {
                name: import_name,
                source: if import_source.is_empty() {
                    None
                } else {
                    Some(import_source)
                },
                line_number,
                kind: import_kind,
            });
        }
    }
    imports
}
