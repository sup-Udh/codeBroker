//! Home-directory and CodeBroker-state-directory resolution.
//!
//! `HOME` is not set on stock Windows shells (cmd.exe, PowerShell without
//! Git/WSL tooling) — every lookup here must fall back to `USERPROFILE`,
//! then the older `HOMEDRIVE`+`HOMEPATH` pair, or callers silently misbehave
//! on Windows (this was the cause of `set_workspace`/`resolve_workspace`
//! never finding the `active_project` pointer on Windows).

use std::path::PathBuf;

/// Testable core: looks up home dir given an arbitrary variable source so
/// tests can inject fake env without mutating real process state.
fn home_dir_with<F: Fn(&str) -> Option<String>>(var: F) -> Option<PathBuf> {
    if let Some(home) = var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }
    if let Some(profile) = var("USERPROFILE") {
        if !profile.is_empty() {
            return Some(PathBuf::from(profile));
        }
    }
    if let (Some(drive), Some(path)) = (var("HOMEDRIVE"), var("HOMEPATH")) {
        if !drive.is_empty() && !path.is_empty() {
            return Some(PathBuf::from(format!("{}{}", drive, path)));
        }
    }
    None
}

/// The current user's home directory: `HOME`, then `USERPROFILE`, then
/// `HOMEDRIVE`+`HOMEPATH`. This is the only place in the repo that should
/// read any of those four environment variables.
pub fn home_dir() -> Option<PathBuf> {
    home_dir_with(|k| std::env::var(k).ok())
}

/// `~/.codebroker` — where the active-project pointer and per-project
/// database live.
pub fn codebroker_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".codebroker"))
}

/// `~/.codebroker/active_project` — written by `codebroker bind` /
/// `set_workspace`, read by every `codebroker-mcp` invocation to find the
/// right database.
pub fn active_project_path() -> Option<PathBuf> {
    codebroker_dir().map(|d| d.join("active_project"))
}

/// `~/.codebroker/openai_api_key` — a local, per-machine fallback for the
/// key so it only has to be set once (via `codebroker bind`) instead of in
/// every shell session. Deliberately NOT read from anywhere in the repo or
/// release archive — this file only ever exists on a user's own machine,
/// written by `bind` itself, never checked in or shipped.
pub fn openai_api_key_path() -> Option<PathBuf> {
    codebroker_dir().map(|d| d.join("openai_api_key"))
}

/// Testable core: env var first, then an already-read file fallback.
fn openai_api_key_with(env_value: Option<String>, file_contents: Option<String>) -> Option<String> {
    if let Some(key) = env_value {
        if !key.is_empty() {
            return Some(key);
        }
    }
    file_contents
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The OpenAI API key to use for semantic tools: `OPENAI_API_KEY` if set in
/// the current shell, otherwise the per-machine file `bind` persists so the
/// key doesn't have to be re-exported every session. Never embedded in any
/// shipped binary or archive — this only ever reads local machine state.
pub fn openai_api_key() -> Option<String> {
    let env_value = std::env::var("OPENAI_API_KEY").ok();
    let file_contents = openai_api_key_path().and_then(|p| std::fs::read_to_string(p).ok());
    openai_api_key_with(env_value, file_contents)
}

/// Persists `key` to `~/.codebroker/openai_api_key` so future commands can
/// pick it up via [`openai_api_key`] without the caller re-exporting it.
/// No-op (returns `Ok(())`) when `key` is empty, so callers can pass through
/// an unconditionally-fetched env var without an extra `is_empty()` check.
pub fn persist_openai_api_key(key: &str) -> std::io::Result<()> {
    if key.is_empty() {
        return Ok(());
    }
    let dir = codebroker_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "could not determine home directory")
    })?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("openai_api_key"), key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_env_value_over_file() {
        let result = openai_api_key_with(Some("sk-env".to_string()), Some("sk-file".to_string()));
        assert_eq!(result, Some("sk-env".to_string()));
    }

    #[test]
    fn falls_back_to_file_when_env_unset() {
        let result = openai_api_key_with(None, Some("sk-file\n".to_string()));
        assert_eq!(result, Some("sk-file".to_string()));
    }

    #[test]
    fn empty_env_value_falls_back_to_file() {
        let result = openai_api_key_with(Some(String::new()), Some("sk-file".to_string()));
        assert_eq!(result, Some("sk-file".to_string()));
    }

    #[test]
    fn none_when_neither_set() {
        assert_eq!(openai_api_key_with(None, None), None);
    }

    #[test]
    fn prefers_home_over_userprofile() {
        let result = home_dir_with(|k| match k {
            "HOME" => Some("/home/alice".to_string()),
            "USERPROFILE" => Some(r"C:\Users\alice".to_string()),
            _ => None,
        });
        assert_eq!(result, Some(PathBuf::from("/home/alice")));
    }

    #[test]
    fn falls_back_to_userprofile_when_home_unset() {
        let result = home_dir_with(|k| match k {
            "USERPROFILE" => Some(r"C:\Users\alice".to_string()),
            _ => None,
        });
        assert_eq!(result, Some(PathBuf::from(r"C:\Users\alice")));
    }

    #[test]
    fn falls_back_to_homedrive_homepath_when_others_unset() {
        let result = home_dir_with(|k| match k {
            "HOMEDRIVE" => Some("C:".to_string()),
            "HOMEPATH" => Some(r"\Users\alice".to_string()),
            _ => None,
        });
        assert_eq!(result, Some(PathBuf::from(r"C:\Users\alice")));
    }

    #[test]
    fn empty_values_are_treated_as_unset() {
        let result = home_dir_with(|k| match k {
            "HOME" => Some(String::new()),
            "USERPROFILE" => Some(r"C:\Users\alice".to_string()),
            _ => None,
        });
        assert_eq!(result, Some(PathBuf::from(r"C:\Users\alice")));
    }

    #[test]
    fn none_when_nothing_set() {
        let result = home_dir_with(|_| None);
        assert_eq!(result, None);
    }

    #[test]
    fn codebroker_dir_joins_dot_codebroker() {
        let home = home_dir();
        if let Some(home) = home {
            assert_eq!(codebroker_dir(), Some(home.join(".codebroker")));
        }
    }
}
