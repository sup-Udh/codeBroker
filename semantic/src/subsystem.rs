use crate::provider::LlmProvider;
use query::subsystem::SubsystemStats;
use storage::Database;

pub struct SubsystemOverviewGenerator<'a> {
    provider: &'a Box<dyn LlmProvider>,
    db: &'a Database,
    model_name: String,
}

impl<'a> SubsystemOverviewGenerator<'a> {
    pub fn new(provider: &'a Box<dyn LlmProvider>, db: &'a Database, model_name: String) -> Self {
        Self { provider, db, model_name }
    }

    pub fn generate_overview(&self, stats: &SubsystemStats) -> Result<String, String> {
        // 1. Cache Lookup
        let mut check_stmt = self.db.conn.prepare(
            "SELECT overview_text FROM subsystem_overviews WHERE subsystem_hash = ?1 AND model_name = ?2"
        ).map_err(|e| e.to_string())?;
        
        if let Ok(overview) = check_stmt.query_row(rusqlite::params![stats.subsystem_hash, self.model_name], |r| r.get::<_, String>(0)) {
            return Ok(overview);
        }

        // 2. Build Prompt
        let mut prompt = String::new();
        prompt.push_str(&format!("Explain the '{}' subsystem within this repository.\n", stats.name));
        prompt.push_str("Based on deterministic graph discovery, here are the core components:\n\n");
        prompt.push_str(&format!("Files:\n{}\n\n", stats.files.join("\n")));
        prompt.push_str(&format!("Symbols:\n{}\n\n", stats.symbols.join("\n")));
        prompt.push_str(&format!("Dependencies (it relies on):\n{}\n\n", stats.dependencies.join("\n")));
        prompt.push_str(&format!("Consumers (they rely on it):\n{}\n\n", stats.consumers.join("\n")));
        prompt.push_str(&format!("Entrypoints / Routes:\n{}\n\n", stats.entrypoints.join("\n")));
        
        prompt.push_str("Provide a structured architectural explanation of this subsystem. Include:\n");
        prompt.push_str("- Purpose\n- Major Components\n- Dependencies & Consumers\n- Data Flow\n- Architectural Summary\n");
        prompt.push_str("Use clear markdown formatting. Do not output anything other than the explanation.\n");

        // 3. Generate
        let system_prompt = "You are an expert software architect analyzing a subsystem within a large codebase. Use the provided deterministic subsystem stats to formulate a comprehensive explanation of its role, components, and impact on the broader system.\n\n";
        let full_prompt = format!("{}{}", system_prompt, prompt);
        let (response, _tokens) = self.provider.generate_summary(&full_prompt, 45, 2048)?;

        // 4. Cache
        let mut insert_stmt = self.db.conn.prepare(
            "INSERT INTO subsystem_overviews (subsystem_name, subsystem_hash, model_name, overview_text) VALUES (?1, ?2, ?3, ?4)"
        ).map_err(|e| e.to_string())?;
        let _ = insert_stmt.execute(rusqlite::params![stats.name, stats.subsystem_hash, self.model_name, response]).map_err(|e| e.to_string())?;

        Ok(response)
    }
}
