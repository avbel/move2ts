---
title: "move2ts --version shows hardcoded version instead of release tag"
category: build-errors
date: 2026-03-22
tags:
  - rust
  - cargo
  - build-rs
  - versioning
  - ci
  - release-pipeline
component: build-system/ci-release-pipeline
severity: minor
symptoms:
  - "`move2ts --version` outputs `0.1.0` instead of the actual release version (e.g. `0.2.5`)"
  - "Cargo.toml version hardcoded while npm packages derived version from git tag"
  - "Version mismatch between the Rust binary and the npm package after installation"
---

## Problem

After installing `move2ts` via npm, running `move2ts --version` reported `0.1.0` instead of the actual release version `0.2.5`. The npm packages correctly used `0.0.0` placeholders stamped from the git tag during CI, but the Rust binary's version was hardcoded in `Cargo.toml` and never updated during releases.

## Root Cause

`Cargo.toml` had `version = "0.1.0"` as a static value. Clap's `#[command(version)]` attribute reads `CARGO_PKG_VERSION`, which is sourced from `Cargo.toml` at compile time. The release pipeline stamped npm `package.json` files from the git tag but never touched `Cargo.toml`, so the binary always reported the stale hardcoded version.

## Solution

Created a `build.rs` that resolves the version at compile time using a priority chain:

1. `MOVE2TS_VERSION` environment variable (set by CI from the git tag)
2. `git describe --tags --abbrev=0` (for local dev builds)
3. `CARGO_PKG_VERSION` from `Cargo.toml` (fallback)

### Files Changed

**`build.rs`** (new):

```rust
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
    if !output.status.success() { return None; }
    let tag = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if tag.is_empty() { return None; }
    Some(strip_v(&tag))
}
```

**`src/cli.rs`**:

```rust
// Before:
#[command(version)]

// After:
#[command(version = env!("MOVE2TS_VERSION"))]
```

**`Cargo.toml`**: version changed to `0.0.0` (placeholder, matching npm pattern).

**`.github/workflows/release.yml`**: added `MOVE2TS_VERSION: ${{ github.ref_name }}` to the build job env.

## Verification

- **Local builds**: `build.rs` reads `git describe --tags`, so `move2ts --version` reports the latest tag (e.g. `0.2.6`).
- **CI release builds**: `MOVE2TS_VERSION` is set from the triggering tag ref, which `build.rs` reads first and strips the `v` prefix.
- **No-git environments**: falls back to `0.0.0` from `Cargo.toml` -- an explicit signal that the version was not resolved.

## Prevention

- **CI version assertion**: add a post-build step that runs `./move2ts --version` and asserts the output matches the git tag before publishing any artifact.
- **Single source of truth**: the git tag is the canonical version. All artifacts (Cargo binary, npm packages, Homebrew formula) derive from it -- never manually edit version fields for a release.
- **Regression test**: an integration test that rejects known placeholder versions would catch future drift.

## Related

- npm version stamping pattern: `release.yml` lines 188-200 (jq rewrites `package.json` from `${{ github.ref_name }}`)
- Cross-reference: both npm and Cargo now use the same `MOVE2TS_VERSION` env var sourced from the git tag
