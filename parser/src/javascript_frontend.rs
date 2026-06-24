use crate::frontend::LanguageFrontend;
use graph::{ImportNode, SymbolNode};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

pub struct JavaScriptFrontend;

impl LanguageFrontend for JavaScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".js") || path.ends_with(".jsx")
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
        let language = tree_sitter_javascript::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(source_code, None)?;

        let mut directive = None;
        if source_code.contains("\"use client\"") || source_code.contains("'use client'") {
            directive = Some("use client".to_string());
        } else if source_code.contains("\"use server\"") || source_code.contains("'use server'") {
            directive = Some("use server".to_string());
        }

        let metadata = graph::models::FileMetadata {
            directive,
            ..Default::default()
        };

        let symbols = extract_js_symbols(&tree, source_code, language.clone(), _path);
        let imports = extract_js_imports(&tree, source_code, language);

        Some((metadata, symbols, imports))
    }
}

fn extract_js_symbols(
    tree: &Tree,
    source_code: &str,
    _language: tree_sitter::Language,
    path: &str,
) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_jsx = path.ends_with(".jsx");
    let query_str = "
        (class_declaration name: (identifier) @type)
        (function_declaration name: (identifier) @function)
        (return_statement (jsx_element) @jsx_render)
        (return_statement (parenthesized_expression (jsx_element) @jsx_render))
        (lexical_declaration 
            (variable_declarator 
                name: (identifier) @function 
                value: (arrow_function)
            )
        )
        (lexical_declaration 
            (variable_declarator 
                name: (identifier) @function 
                value: (call_expression)
            )
        )
    ";

    let language = tree_sitter_javascript::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let mut name_str = name.to_string();
                let mut kind = capture_kind.to_string();
                let mut route_path = None;
                let mut route_method = None;

                if kind == "jsx_render" {
                    name_str = "render".to_string();
                    kind = "jsx_element".to_string();
                } else if kind == "function" {
                    const HTTP_METHODS: [&str; 7] =
                        ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
                    if (filename == "route.js" || filename == "route.jsx")
                        && HTTP_METHODS.contains(&name_str.as_str())
                    {
                        kind = "route".to_string();

                        let mut p = path.to_string();
                        if p.starts_with("./src/app") {
                            p = p.replacen("./src/app", "", 1);
                        } else if p.starts_with("./app") {
                            p = p.replacen("./app", "", 1);
                        } else if p.starts_with("src/app") {
                            p = p.replacen("src/app", "", 1);
                        } else if p.starts_with("app") {
                            p = p.replacen("app", "", 1);
                        }

                        if p.ends_with("/route.js") {
                            p = p.replacen("/route.js", "", 1);
                        } else if p.ends_with("/route.jsx") {
                            p = p.replacen("/route.jsx", "", 1);
                        }

                        if p.is_empty() {
                            p = "/".to_string();
                        }
                        if !p.starts_with('/') {
                            p = format!("/{}", p);
                        }

                        route_path = Some(p);
                        route_method = Some(name_str.clone());
                    } else if name_str.starts_with("use") {
                        kind = "hook".to_string();
                    } else if name_str.ends_with("Provider") {
                        kind = "provider".to_string();
                    } else if is_jsx && name_str.chars().next().unwrap_or('a').is_uppercase() {
                        kind = "component".to_string();
                        if filename == "page.jsx" {
                            kind = "page".to_string();
                        } else if filename == "layout.jsx" {
                            kind = "layout".to_string();
                        }
                    }
                }

                let mut parent = node.parent().unwrap_or(node);

                // The node that actually owns a "body" field (function/class/arrow),
                // captured before `parent` gets widened to its lexical_declaration/export_statement.
                let decl_node = if parent.kind() == "variable_declarator" {
                    parent.child_by_field_name("value")
                } else {
                    Some(parent)
                };

                let mut is_call_expr_assignment = false;
                if parent.kind() == "variable_declarator" {
                    if let Some(value_node) = parent.child_by_field_name("value") {
                        if value_node.kind() == "call_expression" {
                            is_call_expr_assignment = true;
                        }
                    }
                }

                let mut is_exported = false;
                let mut current = parent;
                while let Some(p) = current.parent() {
                    if p.kind() == "export_statement" || p.kind() == "export_clause" {
                        is_exported = true;
                        break;
                    }
                    // A statement_block belongs to an enclosing function/method body.
                    // Anything nested inside one is a local, never a module-level export,
                    // no matter what wraps the enclosing function.
                    if p.kind() == "program" || p.kind() == "statement_block" {
                        break;
                    }
                    current = p;
                }

                if is_call_expr_assignment && kind == "function" {
                    if !is_exported {
                        continue; // Skip indexing this local generic variable
                    }
                    kind = "variable".to_string();
                }

                if parent.kind() == "variable_declarator" {
                    if let Some(lex) = parent.parent() {
                        if lex.kind() == "lexical_declaration" {
                            parent = lex;
                        }
                    }
                }
                if let Some(exp) = parent.parent() {
                    if exp.kind() == "export_statement" {
                        parent = exp;
                    }
                }

                let end_line = parent.end_position().row + 1;

                let signature = decl_node
                    .and_then(|d| d.child_by_field_name("body"))
                    .and_then(|body| {
                        source_code
                            .get(parent.start_byte()..body.start_byte())
                            .map(|s| s.trim_end().to_string())
                    })
                    .filter(|s| !s.is_empty());

                symbols.push(SymbolNode {
                    route_path,
                    route_method,
                    name: name_str,
                    kind,
                    prop_type: None,
                    start_line: parent.start_position().row + 1,
                    end_line,
                    start_byte: parent.start_byte(),
                    end_byte: parent.end_byte(),
                    signature,
                });
            }
        }
    }
    symbols
}

fn extract_js_imports(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    let query_str = "
        (import_statement 
            (import_clause (named_imports (import_specifier name: (identifier) @import)))
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (identifier) @import)
            source: (string (string_fragment) @source)
        )
        (jsx_opening_element (identifier) @jsx_element)
        (jsx_self_closing_element (identifier) @jsx_element)
        (jsx_expression (identifier) @call_name)
        (jsx_expression (member_expression property: (property_identifier) @method_call))
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @method_call))
        (call_expression 
            function: [(identifier) @http_fn (member_expression property: (property_identifier) @http_fn)]
            arguments: (arguments [(string (string_fragment) @http_route) (template_string (string_fragment) @http_route)])
        )
        (member_expression property: (property_identifier) @member_access)
        (string (string_fragment) @route_string)
    ";

    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut import_name = String::new();
        let mut import_source = String::new();
        let mut line_number = 0;

        let mut import_kind = "imports".to_string();

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "import" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "source" {
                    import_source = text.trim().to_string();
                } else if *capture_kind == "jsx_element" {
                    let name = text.trim().to_string();
                    if name.chars().next().unwrap_or('a').is_uppercase() {
                        import_name = name.clone();
                        if name.ends_with("Provider") {
                            import_kind = "renders_provider".to_string();
                        } else {
                            import_kind = "renders_component".to_string();
                        }
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "call_name" {
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    if name.starts_with("use") {
                        import_kind = "consumes_hook".to_string();
                    } else {
                        import_kind = "calls".to_string();
                    }
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "method_call" {
                    // Member-access invocation (obj.foo()). Tracked under a
                    // distinct kind so the linker never resolves it against a
                    // same-named top-level symbol — that bare-name matching is
                    // what produced phantom edges like `query.delete()` ->
                    // exported `DELETE`.
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    import_kind = "method_call".to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "member_access" {
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        import_kind = "MEMBER_ACCESS".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "http_fn" {
                    let fn_name = text.trim().to_string();
                    let is_http = fn_name == "fetch"
                        || fn_name == "get"
                        || fn_name == "post"
                        || fn_name == "put"
                        || fn_name == "delete"
                        || fn_name == "patch"
                        || fn_name == "request";
                    if is_http {
                        // find the string argument in the same match
                        if let Some(route_node) = m
                            .captures
                            .iter()
                            .find(|c| query.capture_names()[c.index as usize] == "http_route")
                        {
                            if let Ok(route) = route_node.node.utf8_text(source_code.as_bytes()) {
                                let r = route.trim().to_string();
                                if r.starts_with('/') {
                                    import_name = r.clone();
                                    import_kind = "HTTP_CALL".to_string();
                                    line_number = node.start_position().row + 1;

                                    let m = if fn_name == "fetch" || fn_name == "request" {
                                        "GET"
                                    } else {
                                        &fn_name
                                    };
                                    import_source = m.to_uppercase();
                                }
                            }
                        }
                    }
                } else if *capture_kind == "route_string" {
                    let val = text.trim().to_string();
                    if val.starts_with('/') && import_kind == "imports" {
                        import_name = val;
                        import_kind = "route_push".to_string();
                        line_number = node.start_position().row + 1;
                    }
                }
            }
        }

        if import_kind == "HTTP_CALL" {
            imports.push(ImportNode {
                name: import_name.clone(),
                source: if import_source.is_empty() {
                    None
                } else {
                    Some(import_source.clone())
                },
                line_number,
                kind: Some(import_kind.clone()),
            });
        } else if !import_name.is_empty() && import_kind != "imports" {
            // We set import_kind to imports initially, but here we don't want to push empty imports. Wait, original logic pushed imports!
            // We should just use the original logic.
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
                kind: Some(import_kind),
            });
        }
    }
    imports
}

#[cfg(test)]
mod call_resolution_tests {
    use super::*;
    use crate::frontend::LanguageFrontend;

    // #2 — call_resolution_fixture: a member-access call (obj.foo()) must be
    // tagged "method_call" so the linker never resolves it to a same-named
    // top-level symbol, while a free call (foo()) stays "calls".
    #[test]
    fn member_calls_tagged_method_call_free_calls_tagged_calls() {
        let src = r#"
            export function GET() {
                helper();
                query.deleteRoom();
            }
            function helper() {}
        "#;
        let (_meta, _symbols, imports) = JavaScriptFrontend
            .parse_and_extract(src, "route.js")
            .expect("parse should succeed");

        let kind_of = |name: &str| {
            imports
                .iter()
                .find(|i| i.name == name)
                .and_then(|i| i.kind.clone())
        };

        assert_eq!(
            kind_of("helper").as_deref(),
            Some("calls"),
            "free call helper() should be a 'calls' edge"
        );
        assert_eq!(
            kind_of("deleteRoom").as_deref(),
            Some("method_call"),
            "member call query.deleteRoom() should be a 'method_call', not 'calls'"
        );
    }
}
