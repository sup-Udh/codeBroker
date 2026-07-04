use crate::flow::variables::VariableState;
use storage::Database;
use graph::models::{SemanticBindingKind, ResolutionEvidence, VariableOrigin};
use crate::semantic::evidence::ResolutionConfidence;
use std::collections::HashMap;
use crate::resolver::index::SymbolIndex;
use crate::resolver::import_resolver::ImportResolver;
use std::sync::Arc;

pub struct VariableFlowEngine {
    pub variables: HashMap<i64, HashMap<String, VariableState>>,
    pub function_returns: HashMap<String, String>, // Function name -> Return type
}

impl VariableFlowEngine {
    pub fn new(db: &Database, symbol_index: Arc<SymbolIndex>, import_resolver: Arc<ImportResolver>) -> Self {
        let mut engine = Self {
            variables: HashMap::new(),
            function_returns: HashMap::new(),
        };
        engine.load_semantic_bindings(db, &symbol_index, &import_resolver);
        // load_constructors before load_imports: an imported name might be a
        // module-level singleton instance rather than a class
        // (`export const inventoryRepository = new InventoryRepository()`),
        // and load_imports needs that instance's own already-inferred
        // constructed type (recorded here, keyed by its *defining* file) to
        // resolve method calls on it — see load_imports below.
        engine.load_constructors(db);
        engine.load_imports(db, &symbol_index, &import_resolver);
        engine.resolve_aliases(db);
        engine
    }

    pub fn get_or_create_var(&mut self, file_id: i64, name: &str) -> &mut VariableState {
        let parts: Vec<&str> = name.split('.').collect();
        let mut var = self.variables
            .entry(file_id)
            .or_default()
            .entry(parts[0].to_string())
            .or_insert_with(|| VariableState::new(file_id, parts[0].to_string()));
            
        for &part in &parts[1..] {
            var = var.get_or_create_field(part);
        }
        var
    }

    pub fn get_var(&self, file_id: i64, name: &str) -> Option<&VariableState> {
        let parts: Vec<&str> = name.split('.').collect();
        let mut var = self.variables.get(&file_id).and_then(|m| m.get(parts[0]))?;
        
        for &part in &parts[1..] {
            var = var.get_field(part)?;
        }
        Some(var)
    }

    fn load_semantic_bindings(&mut self, db: &Database, symbol_index: &SymbolIndex, import_resolver: &ImportResolver) {
        if let Ok(all_bindings) = db.get_all_semantic_bindings() {
            // First pass: register function returns
            for (_, binding) in &all_bindings {
                if binding.kind == SemanticBindingKind::ReturnType {
                    self.function_returns.insert(binding.name.clone(), binding.type_name.clone());
                }
            }

            // Second pass: apply to variables (skip aliases for now)
            for (file_id, binding) in all_bindings {
                match binding.kind {
                    SemanticBindingKind::VarType => {
                        let var = self.get_or_create_var(file_id, &binding.name);
                        var.apply_type(
                            binding.type_name,
                            VariableOrigin::Parameter,
                            ResolutionConfidence::Certain,
                            ResolutionEvidence::ParameterType,
                        );
                    }
                    SemanticBindingKind::FieldType => {
                        let var = self.get_or_create_var(file_id, &binding.name);
                        var.apply_type(
                            binding.type_name,
                            VariableOrigin::Field,
                            ResolutionConfidence::Certain,
                            ResolutionEvidence::FieldType,
                        );
                    }
                    SemanticBindingKind::Assignment => {
                        let ret_type = self.function_returns.get(&binding.type_name).cloned();
                        if let Some(ret_type) = ret_type {
                            let var = self.get_or_create_var(file_id, &binding.name);
                            var.apply_type(
                                ret_type,
                                VariableOrigin::ReturnValue,
                                ResolutionConfidence::Medium,
                                ResolutionEvidence::ReturnFlow,
                            );
                        }
                    }
                    SemanticBindingKind::ImportAlias => {
                        let var = self.get_or_create_var(file_id, &binding.name);
                        if let Some(imported) = import_resolver.resolve(file_id, &binding.type_name) {
                            var.apply_type(
                                imported.name.clone(),
                                VariableOrigin::Import,
                                ResolutionConfidence::Certain,
                                ResolutionEvidence::ImportFlow,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn load_imports(&mut self, db: &Database, symbol_index: &SymbolIndex, import_resolver: &ImportResolver) {
        if let Ok(relationships) = db.get_all_relationships_with_lines() {
            for (_, file_id, name, _, kind, _) in relationships {
                if kind.as_deref() == Some("imports") {
                    if let Some(imported) = import_resolver.resolve(file_id, &name) {
                        // The imported name is usually a class/type, in which
                        // case it IS the effective type. But it might instead
                        // be a module-level singleton instance
                        // (`export const inventoryRepository = new
                        // InventoryRepository()`) — follow it one hop to
                        // what load_constructors (which already ran, see
                        // `new` above) inferred for that instance in its own
                        // defining file, since that's the type
                        // find_method_in_type actually needs.
                        let effective_type = imported
                            .symbol_id
                            .and_then(|sid| symbol_index.get_symbol(sid))
                            .filter(|sym| sym.kind != "type")
                            .and_then(|sym| self.get_var(sym.file_id, &imported.name))
                            .and_then(|v| v.inferred_type.clone())
                            .unwrap_or_else(|| imported.name.clone());

                        let var = self.get_or_create_var(file_id, &name);
                        var.apply_type(
                            effective_type,
                            VariableOrigin::Import,
                            ResolutionConfidence::Certain,
                            ResolutionEvidence::ImportFlow,
                        );
                    }
                }
            }
        }
    }

    fn load_constructors(&mut self, db: &Database) {
        if let Ok(relationships) = db.get_all_relationships_with_lines() {
            for (_, file_id, name, source, kind, _) in relationships {
                let k = kind.as_deref().unwrap_or("imports");
                if k == "new_call" || k == "instantiates" {
                    if let Some(var_name) = source {
                        let var = self.get_or_create_var(file_id, &var_name);
                        var.apply_type(
                            name.clone(),
                            VariableOrigin::Constructor,
                            ResolutionConfidence::High,
                            ResolutionEvidence::ConstructorCall,
                        );
                    }
                } else if k == "calls" || k == "method_call" {
                    if let Some(var_name) = source {
                        // Fallback to the function/method name as a provisional type if we don't know the exact return type
                        let ret_type = self.function_returns.get(&name).cloned().unwrap_or_else(|| name.clone());
                        let var = self.get_or_create_var(file_id, &var_name);
                        var.apply_type(
                            ret_type,
                            VariableOrigin::ReturnValue,
                            ResolutionConfidence::Medium,
                            ResolutionEvidence::ReturnFlow,
                        );
                    }
                }
            }
        }
    }

    fn resolve_aliases(&mut self, db: &Database) {
        // Collect all aliases, destructuring, and object literals
        let mut file_bindings: HashMap<i64, Vec<(String, String, SemanticBindingKind)>> = HashMap::new();
        if let Ok(all_bindings) = db.get_all_semantic_bindings() {
            for (file_id, binding) in all_bindings {
                if matches!(binding.kind, SemanticBindingKind::Alias | SemanticBindingKind::Destructuring | SemanticBindingKind::ObjectLiteral | SemanticBindingKind::Assignment) {
                    file_bindings.entry(file_id).or_default().push((binding.name, binding.type_name, binding.kind));
                }
            }
        }

        for (file_id, bindings) in file_bindings {
            let mut resolved = true;
            let mut passes = 0;
            let max_passes = bindings.len();
            
            while resolved && passes <= max_passes {
                resolved = false;
                passes += 1;
                
                let mut updates = Vec::new();
                for (name, source_name, kind) in &bindings {
                    match kind {
                        SemanticBindingKind::Alias => {
                            let has_type = self.get_var(file_id, name).and_then(|v| v.inferred_type.clone()).is_some();
                            if !has_type {
                                if let Some(source_var) = self.get_var(file_id, source_name) {
                                    if let Some(src_type) = &source_var.inferred_type {
                                        updates.push((name.clone(), src_type.clone(), VariableOrigin::Alias, ResolutionEvidence::Alias));
                                    }
                                }
                            }
                        }
                        SemanticBindingKind::Destructuring => {
                            let has_type = self.get_var(file_id, name).and_then(|v| v.inferred_type.clone()).is_some();
                            if !has_type {
                                let field_path = format!("{}.{}", source_name, name);
                                if let Some(field_var) = self.get_var(file_id, &field_path) {
                                    if let Some(src_type) = &field_var.inferred_type {
                                        updates.push((name.clone(), src_type.clone(), VariableOrigin::Destructuring, ResolutionEvidence::Destructuring));
                                    }
                                }
                            }
                        }
                        SemanticBindingKind::Assignment => {
                            let has_type = self.get_var(file_id, name).and_then(|v| v.inferred_type.clone()).is_some();
                            if !has_type {
                                if let Some(ret_type) = self.function_returns.get(source_name).cloned() {
                                    updates.push((name.clone(), ret_type, VariableOrigin::ReturnValue, ResolutionEvidence::ReturnFlow));
                                } else {
                                    // Provisional fallback to function name
                                    updates.push((name.clone(), source_name.clone(), VariableOrigin::ReturnValue, ResolutionEvidence::ReturnFlow));
                                }
                            }
                        }
                        SemanticBindingKind::ObjectLiteral => {
                            let field_path = format!("{}.{}", source_name, name);
                            let has_type = self.get_var(file_id, &field_path).and_then(|v| v.inferred_type.clone()).is_some();
                            if !has_type {
                                if let Some(source_var) = self.get_var(file_id, name) {
                                    if let Some(src_type) = &source_var.inferred_type {
                                        updates.push((field_path, src_type.clone(), VariableOrigin::ObjectLiteral, ResolutionEvidence::ObjectLiteral));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                
                if !updates.is_empty() {
                    resolved = true;
                    for (alias, src_type, origin, evidence) in updates {
                        let var = self.get_or_create_var(file_id, &alias);
                        var.apply_type(
                            src_type,
                            origin,
                            ResolutionConfidence::Medium,
                            evidence,
                        );
                    }
                }
            }
        }
    }
}
