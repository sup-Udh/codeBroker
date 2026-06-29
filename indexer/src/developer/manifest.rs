use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct RepositoryManifest {
    pub fingerprint: RepositoryFingerprint,
    pub statistics: RepositoryStatistics,
    pub languages: Vec<String>,
    pub frameworks: Vec<FrameworkDetection>,
    pub subsystems: Vec<Subsystem>,
    pub entrypoints: Vec<CategorizedEntrypoint>,
    pub architecture_layers: Vec<ArchitectureLayer>,
    pub hotspots: Hotspots,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepositoryFingerprint {
    pub project_type: String, // e.g. "Full Stack", "Library"
    pub main_frameworks: Vec<String>,
    pub architecture_style: String, // e.g. "Layered", "Monolith"
    pub primary_languages: Vec<String>,
    pub complexity: String, // e.g. "Low", "Medium", "High"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepositoryStatistics {
    pub total_files: usize,
    pub total_symbols: usize,
    pub total_relationships: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FrameworkDetection {
    pub framework: String,
    pub confidence: ConfidenceLevel,
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Subsystem {
    pub name: String,
    pub file_ids: Vec<i64>,
    pub symbol_ids: Vec<i64>,
    pub dependencies: Vec<String>, // Other subsystem names
    pub dependents: Vec<String>,
    pub entrypoint_ids: Vec<i64>,
    pub importance_score: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CategorizedEntrypoint {
    pub category: String, // e.g. "HTTP", "CLI", "Workers"
    pub symbol_id: i64,
    pub file_id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArchitectureLayer {
    pub name: String, // e.g. "Presentation", "Business", "Persistence"
    pub subsystems: Vec<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Hotspots {
    pub highest_pagerank_symbol: Option<i64>,
    pub highest_fan_in_symbol: Option<i64>,
    pub highest_fan_out_symbol: Option<i64>,
    pub most_imported_file: Option<i64>,
    pub most_central_module: Option<String>,
    pub largest_dependency_cluster: Option<String>,
    pub most_connected_symbol: Option<i64>,
    pub architectural_bottleneck: Option<String>,
}
