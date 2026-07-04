use crate::contracts::{GraphPrimitive, ToolManifest};
use resolver::ResolvedEntity;
use storage::Database;

pub struct ReadSymbolSource;

impl ReadSymbolSource {
    pub fn manifest() -> ToolManifest {
        ToolManifest::new(
            "read_symbol_source",
            vec![GraphPrimitive::SemanticNode, GraphPrimitive::Implementation],
        )
    }

    pub fn execute(
        db: &Database,
        symbol: &str,
        file_hint: Option<&str>,
        _include_deps: bool,
    ) -> String {
        // Step 1: Universal Resolver Pipeline
        let resolved = resolver::resolve_symbol(db, symbol, file_hint, None, None);
        let s = match resolved {
            ResolvedEntity::Symbol(s) => s,
            other => return other.to_json_string(),
        };

        // Step 2: Fetch Data
        // Since we have start_byte and end_byte directly on the ResolvedSymbol (which maps to SemanticNode),
        // we do not need ANY extra SQL lookups. The graph identity is all we need.
        let abs_path = s.file_path;
        let mut stale = false;
        
        let source_body = if abs_path.ends_with(".json") {
            "/* JSON symbol – no source body */".to_string()
        } else if let Ok(content) = std::fs::read(&abs_path) {
            let start_b = s.start_byte as usize;
            let end_b = s.end_byte as usize;
            
            // Validate bounds before slicing
            if start_b <= end_b && end_b <= content.len() {
                String::from_utf8_lossy(&content[start_b..end_b]).to_string()
            } else {
                stale = true;
                "/* Source bytes out of bounds (file modified since indexing) */".to_string()
            }
        } else {
            stale = true;
            "/* Error reading file from disk */".to_string()
        };

        // Step 3: Format output
        let json_result = serde_json::json!([{
            "symbol_name": s.name,
            "kind": s.kind,
            "file_path": abs_path,
            "start_line": s.start_line,
            "end_line": s.end_line,
            "source": source_body,
            "stale_index": stale,
            "confidence": s.confidence.score,
        }]);

        serde_json::to_string_pretty(&json_result).unwrap_or_else(|_| "[]".to_string())
    }
}
