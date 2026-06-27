use crate::flow::variables::VariableState;
use storage::Database;
use graph::models::{SemanticBindingKind, ResolutionEvidence, VariableOrigin};
use crate::semantic::evidence::ResolutionConfidence;
use std::collections::HashMap;

pub struct VariableFlowEngine {
    pub variables: HashMap<i64, HashMap<String, VariableState>>,
    pub function_returns: HashMap<String, String>, // Function name -> Return type
}

impl VariableFlowEngine {
    pub fn new(db: &Database) -> Self {
        let mut engine = Self {
            variables: HashMap::new(),
            function_returns: HashMap::new(),
        };
        engine.load_semantic_bindings(db);
        engine.load_constructors(db);
        engine.resolve_aliases(db);
        engine
    }

    fn get_or_create_var(&mut self, file_id: i64, name: &str) -> &mut VariableState {
        self.variables
            .entry(file_id)
            .or_default()
            .entry(name.to_string())
            .or_insert_with(|| VariableState::new(file_id, name.to_string()))
    }

    pub fn get_var(&self, file_id: i64, name: &str) -> Option<&VariableState> {
        self.variables.get(&file_id).and_then(|m| m.get(name))
    }

    fn load_semantic_bindings(&mut self, db: &Database) {
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
                    _ => {}
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
                } else if k == "calls" {
                    if let Some(var_name) = source {
                        let ret_type = self.function_returns.get(&name).cloned();
                        if let Some(ret_type) = ret_type {
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
    }

    fn resolve_aliases(&mut self, db: &Database) {
        // Collect all aliases: file_id -> Vec<(alias, source)>
        let mut file_aliases: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        if let Ok(all_bindings) = db.get_all_semantic_bindings() {
            for (file_id, binding) in all_bindings {
                if binding.kind == SemanticBindingKind::Alias {
                    file_aliases.entry(file_id).or_default().push((binding.name, binding.type_name));
                }
            }
        }

        for (file_id, aliases) in file_aliases {
            let mut resolved = true;
            let mut passes = 0;
            // Maximum passes bounded by the number of aliases (worst case chain)
            let max_passes = aliases.len();
            
            while resolved && passes <= max_passes {
                resolved = false;
                passes += 1;
                
                let mut updates = Vec::new();
                for (alias_name, source_name) in &aliases {
                    let has_type = self.get_var(file_id, alias_name).and_then(|v| v.inferred_type.clone()).is_some();
                    if !has_type {
                        if let Some(source_var) = self.get_var(file_id, source_name) {
                            if let Some(src_type) = &source_var.inferred_type {
                                updates.push((alias_name.clone(), src_type.clone()));
                            }
                        }
                    }
                }
                
                if !updates.is_empty() {
                    resolved = true;
                    for (alias, src_type) in updates {
                        let var = self.get_or_create_var(file_id, &alias);
                        var.apply_type(
                            src_type,
                            VariableOrigin::Alias,
                            ResolutionConfidence::Medium,
                            ResolutionEvidence::Alias,
                        );
                    }
                }
            }
        }
    }
}
