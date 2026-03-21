# move2ts

Rust CLI that parses Sui Move source files and generates type-safe TypeScript wrappers for the @mysten/sui SDK.

## Before Every Commit

Run these commands and fix any issues before committing:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## Project Structure

- `src/ir.rs` — Intermediate representation types (MoveType, ModuleInfo, FunctionInfo, etc.)
- `src/analyzer.rs` — AST extraction from move-compiler, singleton/event detection
- `src/codegen.rs` — TypeScript code generation, type mapping, BCS encoding
- `src/parser.rs` — move-compiler parser wrapper
- `src/driver.rs` — Pipeline orchestration, CLI validation, file I/O
- `src/cli.rs` — clap argument definitions
- `tests/fixtures/` — Move source fixtures for testing
- `tests/ts-check/` — TypeScript compilation validation (real @mysten/sui types)

## Key Conventions

- Move types → TypeScript: see type mapping table in codegen.rs
- External structs (Coin, Kiosk, etc.) default to `TransactionObjectInput`
- Only module's own copy+drop structs get BCS interfaces
- Event fields use snake_case (matching Sui indexer wire format)
- Function param fields use camelCase (TypeScript convention)
- Generic type params with `key` constraint use `TransactionObjectInput`
- Singleton detection excludes copy+drop structs (they can't be on-chain objects)
- Abort-only functions are skipped (deprecated stubs)

## Dependencies

Move compiler crates are git dependencies pinned to a Sui repo commit. To update:
change `rev` in all git deps in `Cargo.toml`.
