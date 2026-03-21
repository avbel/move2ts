use std::process::Command;

fn main() {
    let version = version_from_env()
        .or_else(version_from_git)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=MOVE2TS_VERSION={version}");
    println!("cargo:rerun-if-env-changed=MOVE2TS_VERSION");
    println!("cargo:rerun-if-changed=.git/HEAD");
}

fn strip_v(s: &str) -> String {
    s.strip_prefix('v').unwrap_or(s).to_string()
}

fn version_from_env() -> Option<String> {
    std::env::var("MOVE2TS_VERSION").ok().map(|v| strip_v(&v))
}

fn version_from_git() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tag = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if tag.is_empty() {
        return None;
    }
    Some(strip_v(&tag))
}
