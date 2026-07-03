//! Executable discovery. The ONLY place in the repo that should decide where
//! `codebroker` or `codebroker-mcp` live on disk — every previous call site
//! (`mcp/src/main.rs`'s `run_index`/`run_incremental_index`, `cli/src/main.rs`'s
//! `bind`) hand-rolled its own `current_exe().parent().join(...)` and none of
//! them fell back to PATH, so any install layout other than "both binaries in
//! the same directory" broke silently.

use crate::platform::{current_architecture, current_platform, exe_suffix};
use std::fmt;
use std::path::PathBuf;

pub const CLI_BIN_STEM: &str = "codebroker";
pub const MCP_BIN_STEM: &str = "codebroker-mcp";

/// Descriptive failure for a binary that couldn't be found anywhere searched,
/// in the spirit of "CodeBroker MCP executable could not be located" rather
/// than a bare `fork/exec ...: The system cannot find the file specified`.
#[derive(Debug, Clone)]
pub struct ExecutableError {
    binary_stem: String,
    searched: Vec<PathBuf>,
}

impl fmt::Display for ExecutableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "CodeBroker executable '{}{}' could not be located.",
            self.binary_stem,
            exe_suffix()
        )?;
        writeln!(f, "Platform: {} ({})", current_platform(), current_architecture())?;
        writeln!(f, "Searched:")?;
        for path in &self.searched {
            writeln!(f, "  - {}", path.display())?;
        }
        writeln!(f, "Suggested fix:")?;
        writeln!(f, "  cargo install --path .")?;
        write!(f, "  or download the latest release for your platform")
    }
}

impl std::error::Error for ExecutableError {}

/// Ordered list of directories to search for a sibling CodeBroker binary,
/// built from real process state. Exposed separately from the search loop
/// itself so the loop can be unit tested against a fake directory list.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // 1. Next to the currently running executable — the common case, since
    // `install.sh`/`install.ps1` place both binaries in the same bin dir.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }

    // 2. Every directory on PATH.
    if let Some(path_var) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path_var));
    }

    // 3. `cargo install`'s default bin directory, in case it isn't on PATH.
    if let Some(home) = crate::environment::home_dir() {
        dirs.push(home.join(".cargo").join("bin"));
    }

    dirs
}

/// Pure search: given an ordered list of directories and a binary stem
/// (platform suffix applied internally), returns the first existing match.
fn search_dirs(dirs: &[PathBuf], binary_stem: &str) -> Result<PathBuf, Vec<PathBuf>> {
    let file_name = format!("{}{}", binary_stem, exe_suffix());
    let mut searched = Vec::with_capacity(dirs.len());
    for dir in dirs {
        let candidate = dir.join(&file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        searched.push(candidate);
    }
    Err(searched)
}

/// Locates a CodeBroker binary by stem name (e.g. "codebroker" or
/// "codebroker-mcp"), applying the platform's executable suffix and
/// searching sibling-of-current-exe, then PATH, then `~/.cargo/bin`.
pub struct ExecutableResolver;

impl ExecutableResolver {
    pub fn resolve(binary_stem: &str) -> Result<PathBuf, ExecutableError> {
        search_dirs(&candidate_dirs(), binary_stem).map_err(|searched| ExecutableError {
            binary_stem: binary_stem.to_string(),
            searched,
        })
    }
}

pub fn find_cli_binary() -> Result<PathBuf, ExecutableError> {
    ExecutableResolver::resolve(CLI_BIN_STEM)
}

pub fn find_mcp_binary() -> Result<PathBuf, ExecutableError> {
    ExecutableResolver::resolve(MCP_BIN_STEM)
}

/// The path to the currently running executable itself (thin re-export so
/// callers don't need to reach into `std::env` directly).
pub fn current_executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, b"");
    }

    #[test]
    fn finds_binary_in_first_matching_dir() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let file_name = format!("codebroker-mcp{}", exe_suffix());
        touch(&dir_b.path().join(&file_name));

        let dirs = vec![dir_a.path().to_path_buf(), dir_b.path().to_path_buf()];
        let found = search_dirs(&dirs, "codebroker-mcp").expect("should find binary in dir_b");
        assert_eq!(found, dir_b.path().join(&file_name));
    }

    #[test]
    fn prefers_earlier_directory_when_both_match() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let file_name = format!("codebroker{}", exe_suffix());
        touch(&dir_a.path().join(&file_name));
        touch(&dir_b.path().join(&file_name));

        let dirs = vec![dir_a.path().to_path_buf(), dir_b.path().to_path_buf()];
        let found = search_dirs(&dirs, "codebroker").unwrap();
        assert_eq!(found, dir_a.path().join(&file_name));
    }

    #[test]
    fn returns_every_searched_path_on_miss() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let dirs = vec![dir_a.path().to_path_buf(), dir_b.path().to_path_buf()];

        let err = search_dirs(&dirs, "codebroker-mcp").unwrap_err();
        assert_eq!(err.len(), 2);
    }

    #[test]
    fn error_message_lists_searched_paths_and_fix() {
        let error = ExecutableError {
            binary_stem: "codebroker-mcp".to_string(),
            searched: vec![PathBuf::from("/some/dir/codebroker-mcp")],
        };
        let rendered = error.to_string();
        assert!(rendered.contains("codebroker-mcp"));
        assert!(rendered.contains("/some/dir/codebroker-mcp"));
        assert!(rendered.contains("cargo install"));
    }
}
