//! Cross-platform runtime layer: the only place in the CodeBroker workspace
//! that should read platform-specific environment variables, resolve
//! sibling-binary paths, or branch on `cfg(windows)`/`std::env::consts::OS`.
//! Both the `cli` and `mcp` crates depend on this instead of duplicating the
//! logic (which is how `HOME`-only lookups and bare-binary-name PATH
//! reliance drifted apart and broke on Windows).

pub mod environment;
pub mod executables;
pub mod platform;
pub mod process;
