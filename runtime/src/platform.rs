//! OS/architecture detection, the one place this repo should ever branch on
//! `std::env::consts::OS`/`ARCH`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOS,
    Linux,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Platform::Windows => "Windows",
            Platform::MacOS => "macOS",
            Platform::Linux => "Linux",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
    Other,
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Architecture::X86_64 => "x86_64",
            Architecture::Aarch64 => "aarch64",
            Architecture::Other => std::env::consts::ARCH,
        };
        f.write_str(s)
    }
}

/// The platform this binary was compiled for. Compile-time, not runtime
/// detection — cross-compiled binaries always report the target they were
/// built for, matching `std::env::consts::OS`.
pub fn current_platform() -> Platform {
    match std::env::consts::OS {
        "windows" => Platform::Windows,
        "macos" => Platform::MacOS,
        _ => Platform::Linux,
    }
}

pub fn current_architecture() -> Architecture {
    match std::env::consts::ARCH {
        "x86_64" => Architecture::X86_64,
        "aarch64" => Architecture::Aarch64,
        _ => Architecture::Other,
    }
}

/// `.exe` on Windows, empty everywhere else. Append this — never hardcode a
/// binary name with or without a platform-specific extension anywhere else
/// in the repo.
pub fn exe_suffix() -> &'static str {
    match current_platform() {
        Platform::Windows => ".exe",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_matches_consts_os() {
        let expected = match std::env::consts::OS {
            "windows" => Platform::Windows,
            "macos" => Platform::MacOS,
            _ => Platform::Linux,
        };
        assert_eq!(current_platform(), expected);
    }

    #[test]
    fn exe_suffix_is_exe_only_on_windows() {
        if current_platform() == Platform::Windows {
            assert_eq!(exe_suffix(), ".exe");
        } else {
            assert_eq!(exe_suffix(), "");
        }
    }

    #[test]
    fn architecture_display_is_non_empty() {
        assert!(!current_architecture().to_string().is_empty());
    }
}
