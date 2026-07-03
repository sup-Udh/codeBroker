//! Child-process spawning for CodeBroker's self-relaunch cases (the CLI
//! indexer invoked from the MCP server, the embedding backfill invoked from
//! the CLI). Centralized so stdio handling — which must never inherit, since
//! `codebroker-mcp`'s stdout is a JSON-RPC transport that a child's stray
//! `println!` would corrupt — isn't reimplemented per call site.

use std::path::Path;
use std::process::{Command, Stdio};

/// Runs `binary` with `args`, rooted at `current_dir`, with stdio fully
/// detached (null) so nothing the child prints can land on this process's
/// own stdout/stderr. Returns a descriptive error instead of a bare
/// `io::Error` on spawn failure or non-zero exit.
pub fn run_detached(binary: &Path, args: &[&str], current_dir: &Path) -> Result<(), String> {
    let status = Command::new(binary)
        .args(args)
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| {
            format!(
                "Failed to spawn {} in {}: {}",
                binary.display(),
                current_dir.display(),
                e
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} exited with non-zero status ({}) while running in {}",
            binary.display(),
            status,
            current_dir.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_reports_descriptive_spawn_error() {
        let missing = Path::new("/definitely/not/a/real/codebroker-binary-xyz");
        let cwd = std::env::current_dir().unwrap();
        let err = run_detached(missing, &[], &cwd).unwrap_err();
        assert!(err.contains("Failed to spawn"));
        assert!(err.contains("codebroker-binary-xyz"));
    }
}
