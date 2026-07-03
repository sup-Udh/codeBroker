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

#[cfg(test)]
mod tests {
    use super::*;

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
