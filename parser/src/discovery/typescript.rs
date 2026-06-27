use super::collector::RelationshipCollector;
use super::relationship::{Relationship, RelationshipKind};
use super::visitor::LanguageVisitor;
use graph::{SemanticBinding, SemanticBindingKind};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct TypeScriptVisitor {
    pub is_tsx: bool,
}

impl LanguageVisitor for TypeScriptVisitor {
    fn visit(&self, tree: &Tree, source_code: &str, collector: &mut RelationshipCollector) {
        let language: tree_sitter::Language = if self.is_tsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };

        emit_imports(tree, source_code, &language, collector);
        emit_calls(tree, source_code, &language, collector);
        emit_inheritance(tree, source_code, &language, collector);
        emit_type_refs(tree, source_code, &language, collector);
        emit_decorators(tree, source_code, &language, collector);
        emit_generic_constraints(tree, source_code, &language, collector);
    }

    fn visit_semantic(&self, tree: &Tree, source_code: &str) -> Vec<SemanticBinding> {
        let language: tree_sitter::Language = if self.is_tsx {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
        };
        emit_semantic_bindings(tree, source_code, &language)
    }
}

fn emit_imports(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "
        (import_statement
            (import_clause (named_imports (import_specifier name: (identifier) @import)))
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (identifier) @import)
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (namespace_import (identifier) @ns_import))
            source: (string (string_fragment) @source)
        )
        (export_statement
            (export_clause (export_specifier name: (identifier) @re_export))
            source: (string (string_fragment) @source)
        )
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut source = String::new();
        let mut kind = RelationshipKind::Import;
        let mut line = 0usize;

        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let text = text.trim().to_string();
                match *cn {
                    "import" | "ns_import" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                        kind = RelationshipKind::Import;
                    }
                    "re_export" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                        kind = RelationshipKind::ReExport;
                    }
                    "source" => source = text,
                    _ => {}
                }
            }
        }

        if !name.is_empty() {
            let rel = if source.is_empty() {
                Relationship::new(name, kind, line)
            } else {
                Relationship::new(name, kind, line).with_source(source)
            };
            collector.emit(rel);
        }
    }
}

fn emit_calls(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // ---- Receiver-aware method calls: obj.method() where obj is an identifier ----
    // Emitted first so dedup keeps the version with receiver info.
    let q_meth_recv = "(call_expression function: (member_expression object: (identifier) @receiver property: (property_identifier) @method))";
    if let Ok(query) = Query::new(language, q_meth_recv) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut receiver = String::new();
            let mut method = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "receiver" => receiver = t,
                        "method" => {
                            method = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !method.is_empty() && !crate::utils::is_noisy_call_name(&method) {
                let rel = Relationship::new(method, RelationshipKind::MethodCall, line);
                let rel = if !receiver.is_empty() { rel.with_source(receiver) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- New expressions with variable binding: const x = new Foo() ----
    // Emitted first so dedup keeps the version with variable name.
    let q_new_var = "(lexical_declaration (variable_declarator name: (identifier) @var_name value: (new_expression constructor: (identifier) @constructor)))";
    if let Ok(query) = Query::new(language, q_new_var) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut var_name = String::new();
            let mut constructor = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "var_name" => var_name = t,
                        "constructor" => {
                            constructor = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !constructor.is_empty() && !crate::utils::is_noisy_call_name(&constructor) {
                let rel = Relationship::new(constructor, RelationshipKind::NewCall, line);
                let rel = if !var_name.is_empty() { rel.with_source(var_name) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- this.field.method() — two-level member chain from `this` ----
    // Emitted before fallback so dedup keeps this version (with field context in source).
    // Source is set to "this.<field_name>" to distinguish it from a local var receiver.
    let q_this_meth = "(call_expression function: (member_expression object: (member_expression object: (this) property: (property_identifier) @field_name) property: (property_identifier) @method_name))";
    if let Ok(query) = Query::new(language, q_this_meth) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut field_name = String::new();
            let mut method_name = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "field_name" => field_name = t,
                        "method_name" => {
                            method_name = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !method_name.is_empty() && !field_name.is_empty()
                && !crate::utils::is_noisy_call_name(&method_name)
            {
                let source = format!("this.{}", field_name);
                let rel = Relationship::new(method_name, RelationshipKind::MethodCall, line)
                    .with_source(source);
                collector.emit(rel);
            }
        }
    }

    // ---- Fallback queries (deduplicated against receiver-aware results above) ----
    let query_str = "
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @method_call))
        (member_expression property: (property_identifier) @member_access)
        (new_expression constructor: (identifier) @new_call)
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if crate::utils::is_noisy_call_name(&name) {
                    continue;
                }
                let kind = match *cn {
                    "call_name" => RelationshipKind::Call,
                    "method_call" => RelationshipKind::MethodCall,
                    "member_access" => RelationshipKind::MemberAccess,
                    "new_call" => RelationshipKind::NewCall,
                    _ => continue,
                };
                if !name.is_empty() {
                    collector.emit(Relationship::new(name, kind, line));
                }
            }
        }
    }
}

fn emit_inheritance(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "
        (extends_clause value: (identifier) @extends_class)
        (extends_clause value: (member_expression property: (property_identifier) @extends_member))
        (implements_clause (type_identifier) @implements_type)
        (implements_clause (generic_type name: (type_identifier) @implements_type))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if name.is_empty() {
                    continue;
                }
                let kind = match *cn {
                    "extends_class" | "extends_member" => RelationshipKind::Extends,
                    "implements_type" => RelationshipKind::Implements,
                    _ => continue,
                };
                collector.emit(Relationship::new(name, kind, line));
            }
        }
    }
}

fn emit_type_refs(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // Type annotations: variable declarations, function params, return types
    let query_str = "
        (type_annotation (type_identifier) @type_ref)
        (type_annotation (generic_type name: (type_identifier) @type_ref))
        (type_annotation (predefined_type) @builtin_type)
        (type_parameters (type_parameter name: (type_identifier) @type_param))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if *cn == "builtin_type" || *cn == "type_param" {
                continue;
            }
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !is_ts_builtin_type(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::TypeRef, line));
                }
            }
        }
    }
}

fn emit_decorators(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // TypeScript decorators: @Injectable(), @Component({...}), @Get("/path")
    let query_str = "
        (decorator (identifier) @decorator_name)
        (decorator (call_expression function: (identifier) @decorator_name))
        (decorator (call_expression function: (member_expression property: (property_identifier) @decorator_name)))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() {
                    collector.emit(Relationship::new(name, RelationshipKind::Annotation, line));
                }
            }
        }
    }
}

fn emit_generic_constraints(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // function foo<T extends Serializable>()
    let query_str = "
        (constraint (type_identifier) @constraint)
        (constraint (generic_type name: (type_identifier) @constraint))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !is_ts_builtin_type(&name) {
                    collector.emit(Relationship::new(
                        name,
                        RelationshipKind::GenericConstraint,
                        line,
                    ));
                }
            }
        }
    }
}

fn emit_semantic_bindings(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
) -> Vec<SemanticBinding> {
    let mut bindings = Vec::new();

    // ── Variable type annotations: const x: Type = ... (plain and generic) ────
    let q_var_type = "
        (lexical_declaration (variable_declarator name: (identifier) @var_name type: (type_annotation (type_identifier) @type_name)))
        (variable_declaration (variable_declarator name: (identifier) @var_name type: (type_annotation (type_identifier) @type_name)))
        (lexical_declaration (variable_declarator name: (identifier) @var_name type: (type_annotation (generic_type name: (type_identifier) @type_name))))
        (variable_declaration (variable_declarator name: (identifier) @var_name type: (type_annotation (generic_type name: (type_identifier) @type_name))))
    ";
    if let Ok(query) = Query::new(language, q_var_type) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut var_name = String::new();
            let mut type_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "var_name" => var_name = text.trim().to_string(),
                        "type_name" => type_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            let useful = !is_ts_builtin_type(&type_name) || is_js_receiver_type(&type_name);
            if !var_name.is_empty() && !type_name.is_empty() && useful {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::VarType,
                    name: var_name,
                    type_name,
                    context: None,
                });
            }
        }
    }

    // ── Function / method return type annotations ────────────────────────────
    let q_ret = "
        (function_declaration name: (identifier) @func_name return_type: (type_annotation (type_identifier) @return_type))
        (method_definition name: (property_identifier) @func_name return_type: (type_annotation (type_identifier) @return_type))
    ";
    if let Ok(query) = Query::new(language, q_ret) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut func_name = String::new();
            let mut return_type = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "func_name" => func_name = text.trim().to_string(),
                        "return_type" => return_type = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !func_name.is_empty() && !return_type.is_empty() && !is_ts_builtin_type(&return_type) {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::ReturnType,
                    name: func_name,
                    type_name: return_type,
                    context: None,
                });
            }
        }
    }

    // ── Arrow function return types: const f = (): Type => ... ───────────────
    let q_arrow_ret = "(lexical_declaration (variable_declarator name: (identifier) @func_name value: (arrow_function return_type: (type_annotation (type_identifier) @return_type))))";
    if let Ok(query) = Query::new(language, q_arrow_ret) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut func_name = String::new();
            let mut return_type = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "func_name" => func_name = text.trim().to_string(),
                        "return_type" => return_type = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !func_name.is_empty() && !return_type.is_empty() && !is_ts_builtin_type(&return_type) {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::ReturnType,
                    name: func_name,
                    type_name: return_type,
                    context: None,
                });
            }
        }
    }

    // ── Class field types: class C { private db: Database } (plain and generic)─
    let q_field = "
        (class_declaration name: (type_identifier) @class_name body: (class_body (public_field_definition name: (property_identifier) @field_name type: (type_annotation (type_identifier) @field_type))))
        (class_declaration name: (type_identifier) @class_name body: (class_body (public_field_definition name: (property_identifier) @field_name type: (type_annotation (generic_type name: (type_identifier) @field_type)))))
    ";
    if let Ok(query) = Query::new(language, q_field) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut class_name = String::new();
            let mut field_name = String::new();
            let mut field_type = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "class_name" => class_name = text.trim().to_string(),
                        "field_name" => field_name = text.trim().to_string(),
                        "field_type" => field_type = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            let useful = !is_ts_builtin_type(&field_type) || is_js_receiver_type(&field_type);
            if !field_name.is_empty() && !field_type.is_empty() && useful {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::FieldType,
                    name: field_name,
                    type_name: field_type,
                    context: if class_name.is_empty() { None } else { Some(class_name) },
                });
            }
        }
    }

    // ── Function/method parameter type annotations: (param: Type) ────────────
    // Captures `res: Response`, `req: Request`, etc. so that method calls on
    // parameters (like res.status()) can be receiver-resolved to their declared type.
    let q_param = "
        (function_declaration parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (type_identifier) @param_type))))
        (method_definition parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (type_identifier) @param_type))))
        (arrow_function parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (type_identifier) @param_type))))
        (function_declaration parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (generic_type name: (type_identifier) @param_type)))))
        (method_definition parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (generic_type name: (type_identifier) @param_type)))))
        (arrow_function parameters: (formal_parameters (required_parameter pattern: (identifier) @param_name type: (type_annotation (generic_type name: (type_identifier) @param_type)))))
    ";
    if let Ok(query) = Query::new(language, q_param) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut param_name = String::new();
            let mut param_type = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "param_name" => param_name = text.trim().to_string(),
                        "param_type" => param_type = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            let useful = !is_ts_builtin_type(&param_type) || is_js_receiver_type(&param_type);
            if !param_name.is_empty() && !param_type.is_empty() && useful {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::VarType,
                    name: param_name,
                    type_name: param_type,
                    context: None,
                });
            }
        }
    }

    // ── Alias assignments: const x = y (bare identifier RHS) ────────────────
    let q_alias = "
        (lexical_declaration (variable_declarator name: (identifier) @alias_name value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (identifier) @alias_name value: (identifier) @source_name))
        (assignment_expression left: (identifier) @alias_name right: (identifier) @source_name)
    ";
    if let Ok(query) = Query::new(language, q_alias) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut alias_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "alias_name" => alias_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !alias_name.is_empty() && !source_name.is_empty() && alias_name != source_name {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::Alias,
                    name: alias_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Assignments from function calls: const x = foo() ────────────────────
    let q_assign_call = "
        (lexical_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (identifier) @source_name)))
        (variable_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (identifier) @source_name)))
        (assignment_expression left: (identifier) @assign_name right: (call_expression function: (identifier) @source_name))
        (lexical_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (member_expression property: (property_identifier) @source_name))))
        (variable_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (member_expression property: (property_identifier) @source_name))))
        (assignment_expression left: (identifier) @assign_name right: (call_expression function: (member_expression property: (property_identifier) @source_name)))
    ";
    if let Ok(query) = Query::new(language, q_assign_call) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut assign_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "assign_name" => assign_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !assign_name.is_empty() && !source_name.is_empty() {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::Assignment,
                    name: assign_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Destructuring: const { login } = auth ──────────────────────────────
    let q_destructuring = "
        (lexical_declaration (variable_declarator name: (object_pattern (shorthand_property_identifier_pattern) @destruct_name) value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (object_pattern (shorthand_property_identifier_pattern) @destruct_name) value: (identifier) @source_name))
        (lexical_declaration (variable_declarator name: (object_pattern (pair_pattern value: (identifier) @destruct_name)) value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (object_pattern (pair_pattern value: (identifier) @destruct_name)) value: (identifier) @source_name))
    ";
    if let Ok(query) = Query::new(language, q_destructuring) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut destruct_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "destruct_name" => destruct_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !destruct_name.is_empty() && !source_name.is_empty() {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::Destructuring,
                    name: destruct_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Object literal: const api = { login(){} } ──────────────────────────
    let q_obj_literal = "
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (pair key: (property_identifier) @prop_name))))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (pair key: (property_identifier) @prop_name))))
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (shorthand_property_identifier) @prop_name)))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (shorthand_property_identifier) @prop_name)))
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (method_definition name: (property_identifier) @prop_name))))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (method_definition name: (property_identifier) @prop_name))))
    ";
    if let Ok(query) = Query::new(language, q_obj_literal) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut obj_name = String::new();
            let mut prop_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "obj_name" => obj_name = text.trim().to_string(),
                        "prop_name" => prop_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !obj_name.is_empty() && !prop_name.is_empty() {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::ObjectLiteral,
                    name: prop_name,
                    type_name: obj_name,
                    context: None,
                });
            }
        }
    }

    bindings
}

fn is_ts_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "string" | "number" | "boolean" | "void" | "never" | "any" | "unknown"
        | "null" | "undefined" | "object" | "symbol" | "bigint"
        | "String" | "Number" | "Boolean" | "Object" | "Array" | "Promise"
        | "Record" | "Partial" | "Required" | "Readonly" | "Pick" | "Omit"
        | "Exclude" | "Extract" | "NonNullable" | "ReturnType" | "InstanceType"
        | "Parameters" | "ConstructorParameters" | "ThisType" | "Error"
        | "T" | "K" | "V" | "E" | "R" | "U" | "S"
    )
}

/// Types that are JS runtime receiver objects (should be stored as semantic bindings
/// even if `is_ts_builtin_type` filters them), so method calls on them resolve to Builtin.
fn is_js_receiver_type(name: &str) -> bool {
    matches!(
        name,
        "Array" | "Map" | "Set" | "WeakMap" | "WeakSet" | "Promise"
        | "String" | "Number" | "Boolean" | "Object" | "Error"
        | "Date" | "RegExp" | "Symbol" | "BigInt"
    )
}

#[cfg(test)]
mod ts_visitor_tests {
    use super::*;
    use crate::discovery::collector::RelationshipCollector;

    fn parse_and_collect(src: &str) -> Vec<graph::RelationshipNode> {
        let language: tree_sitter::Language =
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut collector = RelationshipCollector::new();
        TypeScriptVisitor { is_tsx: false }.visit(&tree, src, &mut collector);
        collector.into_relationship_nodes()
    }

    #[test]
    fn named_import_produces_import_edge() {
        let src = r#"import { User } from "./user";"#;
        let rels = parse_and_collect(src);
        let edge = rels.iter().find(|r| r.name == "User" && r.kind.as_deref() == Some("imports"));
        assert!(edge.is_some(), "import edge expected; got {rels:?}");
        assert_eq!(edge.unwrap().source.as_deref(), Some("./user"));
    }

    #[test]
    fn class_extends_produces_extends_edge() {
        let src = "class Child extends Parent {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Parent" && r.kind.as_deref() == Some("extends")));
    }

    #[test]
    fn implements_produces_implements_edge() {
        let src = "class Service implements AuthProvider {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "AuthProvider" && r.kind.as_deref() == Some("implements")));
    }

    #[test]
    fn type_annotation_produces_type_ref() {
        let src = "const user: User = getUser();";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "User" && r.kind.as_deref() == Some("type_ref")));
    }

    #[test]
    fn decorator_produces_annotation_edge() {
        let src = "@Injectable()\nclass MyService {}";
        let rels = parse_and_collect(src);
        assert!(
            rels.iter().any(|r| r.name == "Injectable" && r.kind.as_deref() == Some("annotation")),
            "decorator should produce annotation edge; got {rels:?}"
        );
    }

    #[test]
    fn generic_constraint_produces_edge() {
        let src = "function serialize<T extends Serializable>(val: T): string { return ''; }";
        let rels = parse_and_collect(src);
        assert!(
            rels.iter().any(|r| r.name == "Serializable" && r.kind.as_deref() == Some("generic_constraint")),
            "generic constraint should produce edge; got {rels:?}"
        );
    }

    #[test]
    fn new_expression_produces_new_call() {
        let src = "const x = new MyClass();";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "MyClass" && r.kind.as_deref() == Some("new_call")));
    }

    #[test]
    fn re_export_produces_re_export_edge() {
        let src = r#"export { Foo } from "./foo";"#;
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Foo" && r.kind.as_deref() == Some("re_export")));
    }
}
