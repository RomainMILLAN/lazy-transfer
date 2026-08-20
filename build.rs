use chrono::{DateTime, SecondsFormat, Utc};
use std::process::Command;

fn main() {
    // Git commit hash
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=LT_GIT_COMMIT={commit}");

    println!("cargo:rustc-env=LT_BUILD_DATE={}", build_date());

    // OS and arch are deliberately NOT stamped here. In a build script,
    // `std::env::consts` describes the *host*, so every cross-compiled binary
    // would claim the runner's architecture. The binary reads those consts
    // itself (see `long_version` in src/main.rs), where they describe the target
    // by construction.

    // `.git/HEAD` alone is not enough: a new commit on the same branch rewrites
    // `.git/refs/heads/<branch>`, not `HEAD`.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
}

/// Build date, UTC, ISO 8601. Honours `SOURCE_DATE_EPOCH` — the convention
/// distributions use to make a build reproducible — and falls back to now.
fn build_date() -> String {
    let stamped = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .and_then(|secs| DateTime::from_timestamp(secs, 0));

    stamped
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}
