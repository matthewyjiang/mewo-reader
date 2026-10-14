use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=MEWO_VERSION");
    println!("cargo:rerun-if-env-changed=MEWO_COMMIT");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let git_dir = Path::new(&manifest_dir).join("../.git");
    if git_dir.is_dir() {
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
        println!(
            "cargo:rerun-if-changed={}",
            git_dir.join("packed-refs").display()
        );
        println!("cargo:rerun-if-changed={}", git_dir.join("refs").display());
    }

    let version = nonempty(std::env::var("MEWO_VERSION").ok())
        .or_else(|| git_version(&manifest_dir))
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()));
    let commit = nonempty(std::env::var("MEWO_COMMIT").ok())
        .or_else(|| git_commit(&manifest_dir))
        .unwrap_or_default();

    rustc_env("MEWO_VERSION", &version);
    rustc_env("MEWO_COMMIT", &commit);
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// `git describe --tags --always --dirty`, with one leading `v` removed.
fn git_version(dir: &str) -> Option<String> {
    let described = git(dir, &["describe", "--tags", "--always", "--dirty"])?;
    Some(
        described
            .strip_prefix('v')
            .unwrap_or(&described)
            .to_string(),
    )
}

fn git_commit(dir: &str) -> Option<String> {
    git(dir, &["rev-parse", "--short=7", "HEAD"])
}

fn git(dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn rustc_env(key: &str, value: &str) {
    if value.contains(['\n', '\r']) {
        panic!("{key} must be a single line");
    }
    println!("cargo:rustc-env={key}={value}");
}
