use ignore::{WalkBuilder, overrides::OverrideBuilder};
use std::path::Path;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::fs;

pub fn collect_files(root_dir: &str) -> Vec<String> {
    let mut valid_files = Vec::new();
    let mut seen_hashes = HashSet::new();

    let mut overrides = OverrideBuilder::new(root_dir);
    // Ignore common build output, dependency, and vendor directories.
    // `target/` in particular can be multiple GB on a Rust workspace and
    // contains no source symbols worth indexing.
    let _ = overrides.add("!**/node_modules/**");
    let _ = overrides.add("!**/dist/**");
    let _ = overrides.add("!**/build/**");
    let _ = overrides.add("!**/.next/**");
    let _ = overrides.add("!**/out/**");
    let _ = overrides.add("!**/.codebroker/**");
    let _ = overrides.add("!**/target/**");
    let _ = overrides.add("!**/vendor/**");
    let _ = overrides.add("!**/.venv/**");
    let _ = overrides.add("!**/venv/**");
    let _ = overrides.add("!**/__pycache__/**");
    let _ = overrides.add("!**/.pytest_cache/**");
    let _ = overrides.add("!**/.git/**");
    let _ = overrides.add("!**/.vexp/**");
    let _ = overrides.add("!**/.vexp_backup/**");

    let mut builder = WalkBuilder::new(root_dir);
    if let Ok(ov) = overrides.build() {
        builder.overrides(ov);
    }
    
    let walker = builder.build(); // automatically ignores the .gitingore files the walkbuilder

    for result in walker {
        if let Ok(entry) = result {
            let path = entry.path();

            if path.is_file() {
                if is_supported_file(path) {
                    // Deduplicate by content hash
                    if let Ok(content) = fs::read(path) {
                        let mut hasher = DefaultHasher::new();
                        content.hash(&mut hasher);
                        let file_hash = hasher.finish();

                        if !seen_hashes.contains(&file_hash) {
                            seen_hashes.insert(file_hash);
                            if let Some(path_str) = path.to_str() {
                                valid_files.push(path_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    valid_files
}

fn is_supported_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if let Some(ext_str) = ext.to_str() {
            return ext_str == "rs" || ext_str == "ts" || ext_str == "tsx" || ext_str == "py" || ext_str == "js" || ext_str == "jsx" || ext_str == "json" || ext_str == "toml" || ext_str == "txt" || ext_str == "yml" || ext_str == "yaml" || ext_str == "vue" || ext_str == "svelte";
        }
    }
    
    // Also allow files without extensions or specific names
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if file_name == "Dockerfile" {
        return true;
    }
    false
}