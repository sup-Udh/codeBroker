use std::collections::HashMap;
use storage::Database;

/// TypeGraph manages the inheritance, implements, traits, and interface hierarchies
/// independently from the lexical SymbolIndex.
pub struct TypeGraph {
    pub parents: HashMap<i64, Vec<i64>>,
    pub children: HashMap<i64, Vec<i64>>,
}

impl TypeGraph {
    pub fn new() -> Self {
        Self {
            parents: HashMap::new(),
            children: HashMap::new(),
        }
    }
    
    pub fn build(_db: &Database) -> Result<Self, String> {
        let parents: HashMap<i64, Vec<i64>> = HashMap::new();
        let children: HashMap<i64, Vec<i64>> = HashMap::new();

        // Query implements, extends, inherits, etc.
        // Wait, to build this, we need to map names to IDs.
        // But the TypeGraph itself might just store IDs if we pre-resolve names.
        
        // We will need SymbolIndex to resolve names to IDs.
        
        Ok(Self { parents, children })
    }

    pub fn get_ancestors(&self, start_id: i64) -> Vec<i64> {
        let mut ancestors = Vec::new();
        let mut queue = vec![start_id];
        let mut visited = std::collections::HashSet::new();
        
        while let Some(current) = queue.pop() {
            if visited.insert(current) {
                if let Some(parents) = self.parents.get(&current) {
                    for &parent in parents {
                        ancestors.push(parent);
                        queue.push(parent);
                    }
                }
            }
        }
        
        ancestors
    }
}
