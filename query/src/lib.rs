pub mod concepts;
pub mod context;
pub mod duplicates;
pub mod engine;
pub mod graph;
pub mod metrics;
pub mod response;
pub mod retrieval;
pub mod subsystem;
pub mod validation;

/// Universally normalizes paths (handling Windows `\` and Unix `/`) for scope matching.
pub fn path_matches_scope(path: &str, scope: &str) -> bool {
    let normalized_path = path.replace('\\', "/");
    let normalized_scope = scope.replace('\\', "/");
    normalized_path.contains(&normalized_scope)
}
