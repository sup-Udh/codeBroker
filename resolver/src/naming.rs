use storage::Database;

pub struct CanonicalNameResolver;

impl CanonicalNameResolver {
    /// Takes a subsystem string input (e.g., "authentication", "AUTH", "packages/features/auth")
    /// and resolves it into a single canonical ID / name.
    pub fn resolve_subsystem_name(input: &str) -> String {
        let mut canonical = input.trim().to_lowercase();
        // Remove common prefixes
        canonical = canonical.trim_start_matches("packages/").to_string();
        canonical = canonical.trim_start_matches("features/").to_string();
        canonical = canonical.trim_start_matches("src/").to_string();
        canonical = canonical.trim_start_matches("app/").to_string();
        
        // Strip trailing slashes
        canonical = canonical.trim_end_matches('/').to_string();

        // Handle common aliases
        match canonical.as_str() {
            "authentication" | "authenticate" | "login" | "signin" | "signup" => "auth".to_string(),
            "database" | "db" | "postgres" | "sql" => "database".to_string(),
            "api" | "routes" | "endpoints" => "api".to_string(),
            "ui" | "components" | "frontend" | "views" => "ui".to_string(),
            "utils" | "helpers" | "common" | "shared" => "utils".to_string(),
            "cfg" | "config" | "configuration" => "config".to_string(),
            _ => canonical,
        }
    }

    /// Normalizes file paths (e.g., resolving backslashes, stripping project root).
    pub fn normalize_path(db: &Database, q: &str) -> String {
        let mut normalized = q.trim().replace('\\', "/");
        let root = db.project_root.replace('\\', "/");
        
        if normalized.starts_with(&root) {
            normalized = normalized[root.len()..].to_string();
        }
        
        normalized
            .trim_start_matches('/')
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string()
    }
}
