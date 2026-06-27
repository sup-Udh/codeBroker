use std::collections::HashMap;
use super::evidence::{ResolutionConfidence, SemanticEvidence};

/// A type binding for a single variable / expression.
#[derive(Debug, Clone)]
pub struct TypeBound {
    pub type_name: String,
    pub evidence: SemanticEvidence,
    pub confidence: ResolutionConfidence,
}

/// All semantic facts known about one file, built from DB semantic_bindings
/// and from constructor-binding relationships during the resolver pre-pass.
#[derive(Debug, Default, Clone)]
pub struct FileSemantics {
    /// variable name → type binding (annotations > constructors > aliases > propagation)
    pub var_types: HashMap<String, TypeBound>,
    /// function / method name → return type name
    pub return_types: HashMap<String, String>,
    /// field name → type name (across all classes in the file; first occurrence wins)
    pub field_types: HashMap<String, String>,
    /// alias_name → original_name (for alias propagation)
    pub aliases: HashMap<String, String>,
}

impl FileSemantics {
    /// Resolve a variable to its type, walking alias chains up to depth 5.
    /// Returns (resolved_var_name, &TypeBound) on success.
    pub fn resolve_var_type(&self, var_name: &str) -> Option<&TypeBound> {
        let mut name = var_name;
        for _ in 0..5 {
            if let Some(bound) = self.var_types.get(name) {
                return Some(bound);
            }
            match self.aliases.get(name) {
                Some(target) => name = target.as_str(),
                None => return None,
            }
        }
        None
    }

    /// Resolve a `this.field` or `self.field` receiver to a type name using
    /// the file-level field type index.
    pub fn resolve_field_type(&self, field_name: &str) -> Option<&str> {
        self.field_types.get(field_name).map(String::as_str)
    }

    /// Return type for the named function if known.
    pub fn resolve_return_type(&self, func_name: &str) -> Option<&str> {
        self.return_types.get(func_name).map(String::as_str)
    }
}
