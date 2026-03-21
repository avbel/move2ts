---
title: Using move-compiler as a git dependency for Move source parsing
category: architecture
date: 2026-03-21
tags: [move-compiler, sui, git-dependency, parser, ast]
module: parser
symptom: Need to parse Sui Move source files without vendoring or cloning the Sui repo
root_cause: move-compiler crate is not published to crates.io — only available in the MystenLabs/sui monorepo
---

## Problem

The `move-compiler` crate (which contains the Move parser, lexer, and AST types) is not published to crates.io. It lives inside the MystenLabs/sui monorepo alongside ~200 other crates in a Cargo workspace.

Initial approaches considered:
- **Path dependency to local clone** — breaks CI, requires Sui repo on every machine
- **Vendoring the crates** — complex (16+ transitive deps), requires manual Cargo.toml stripping, hard to update

## Solution

Use **git dependencies** pinned to a specific commit:

```toml
[dependencies]
move-compiler = { git = "https://github.com/MystenLabs/sui.git", rev = "a8b4775b33" }
move-ir-types = { git = "https://github.com/MystenLabs/sui.git", rev = "a8b4775b33" }
move-symbol-pool = { git = "https://github.com/MystenLabs/sui.git", rev = "a8b4775b33" }
move-command-line-common = { git = "https://github.com/MystenLabs/sui.git", rev = "a8b4775b33" }
move-core-types = { git = "https://github.com/MystenLabs/sui.git", rev = "a8b4775b33" }
```

Cargo automatically resolves all transitive workspace dependencies from the same git repo. No vendoring, no local clone, no workspace configuration needed.

**Updating Move version = changing one `rev` value.** Cargo caches git checkouts so subsequent builds don't re-download.

## Key API

The parser entry point is `parse_file_string` at `move-compiler/src/parser/syntax.rs`:

```rust
pub fn parse_file_string(
    env: &CompilationEnv,
    file_hash: FileHash,
    input: &str,
    package: Option<Symbol>,
) -> Result<Vec<Definition>, Diagnostics>
```

Create `CompilationEnv` with `Edition::E2024_BETA` for Sui Move 2024 edition. The parser is error-recovering — it always returns `Ok` with partial results. Only lexer errors return `Err`.

**Create a fresh `CompilationEnv` per file** to prevent diagnostic leakage between files.

## Prevention

- Commit `Cargo.lock` for reproducible builds (the git dependency resolves transitive deps which could change)
- Pin to a specific commit SHA, not a tag (tags can be moved)
- First build is slow (~45s) due to compiling the full move-compiler dependency tree; subsequent builds use Cargo cache
