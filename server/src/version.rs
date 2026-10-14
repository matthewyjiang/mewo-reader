//! Identity compiled into the binary.
//!
//! `MEWO_VERSION` is the release tag with the leading `v` removed.
//! `MEWO_COMMIT` is the short git sha. The server image workflow sets both.
//! Empty or unset falls back to the crate version and no commit.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct VersionInfo {
    pub version: &'static str,
    pub commit: Option<&'static str>,
}

pub fn version() -> &'static str {
    option_env!("MEWO_VERSION")
        .filter(|value| !value.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn commit() -> Option<&'static str> {
    option_env!("MEWO_COMMIT").filter(|value| !value.is_empty())
}

pub fn current() -> VersionInfo {
    VersionInfo {
        version: version(),
        commit: commit(),
    }
}
