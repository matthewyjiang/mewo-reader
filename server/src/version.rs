//! Identity compiled into the binary.
//!
//! `build.rs` resolves the values. `MEWO_VERSION` and `MEWO_COMMIT` win when
//! the build sets them (the release image passes the tag with `v` removed and
//! the short sha). Otherwise the version is `git describe --tags --always
//! --dirty` with one leading `v` removed, and the commit is a 7-character sha.
//! If git is unavailable, the version is the crate version and there is no commit.

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
