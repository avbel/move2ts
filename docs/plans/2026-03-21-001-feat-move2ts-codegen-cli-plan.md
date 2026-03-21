---
title: "feat: Move-to-TypeScript Code Generator CLI"
type: feat
status: active
date: 2026-03-21
origin: docs/brainstorms/2026-03-21-move2ts-brainstorm.md
deepened: 2026-03-21
---

# feat: Move-to-TypeScript Code Generator CLI

## Enhancement Summary

**Deepened on:** 2026-03-21
**Agents used:** architecture-strategist, performance-oracle, security-sentinel, code-simplicity-reviewer, pattern-recognition-specialist, kieran-typescript-reviewer, deployment-verification-agent, best-practices-researcher, framework-docs-researcher, rust-design-patterns

### Key Improvements
1. **SDK type correction:** Use `TransactionObjectInput` from `@mysten/sui` instead of manual `string | TransactionResult`. All wrappers return `TransactionResult`.
2. **Vendor move crates** instead of cloning Sui repo — eliminates CI fragility, reduces build time 60-80%
3. **Define `MoveType` as recursive enum** — the central IR type, not stringly-typed
4. **Defer package ID validation to call time** — module-level throws break testing/tree-shaking
5. **CI fixes:** pin GH Actions to SHAs, fix cross-compilation, complete npm publish pipeline
6. **Simplified file structure:** 4 source files instead of 10+

### Corrections from Research
- **Entry functions CAN receive `TransactionResult` in PTBs** — the entry/public distinction is Move-level, not PTB-level. Object args should use `TransactionObjectInput` for all functions. *(User originally requested string-only for entry — flagged for re-evaluation)*
- **`Option<T>` should map to `T | null`** (not `T | undefined`) — the SDK's `tx.pure.option()` uses `null` for absent values
- **`u64`/`u128`/`u256` accept `string | number | bigint`** in the SDK, but generated code intentionally narrows to `bigint` for type safety — callers pass `bigint`, the SDK handles conversion internally

---

## Overview

A Rust CLI tool that parses Sui Move source files (`.move`) using the `move-compiler` parser crate and generates type-safe TypeScript functions for calling `entry` and `public` methods via the `@mysten/sui` TypeScript SDK. Includes GitHub Actions CI for cross-platform binary releases and npm publishing.

## Problem Statement / Motivation

All existing Sui TypeScript SDKs (sui-trading-sdk, sui-defi-sdk, byz-move-launchpad-sdk-ts) are hand-written — manually defining `moveCall` targets as string templates and constructing arguments by hand. This is tedious, error-prone, and falls out of sync with contract changes. Automating this eliminates a class of bugs and saves significant development time. (see brainstorm: docs/brainstorms/2026-03-21-move2ts-brainstorm.md)

## Proposed Solution

### Architecture

```
┌─────────────┐   ┌──────────────┐   ┌───────────────┐   ┌──────────────┐
│  CLI (clap)  │──>│  Parser      │──>│  Analyzer     │──>│  TS Codegen  │
│  src/main.rs │   │  src/parser  │   │  src/analyzer │   │  src/codegen │
│  src/cli.rs  │   │  .rs         │   │  .rs          │   │  .rs         │
└─────────────┘   └──────────────┘   └───────────────┘   └──────────────┘
      │                                                          │
      └─────────── src/driver.rs (orchestration) ────────────────┘
```

**Crate dependencies — git deps pinned to Sui commit:**

Git dependencies from the MystenLabs/sui repo, pinned by commit rev:
- `move-compiler` — parser + AST types
- `move-ir-types` — `Spanned<T>`, `Loc`, `sp!` macro
- `move-symbol-pool` — `Symbol` interned strings
- `move-command-line-common` — `FileHash`
- `move-core-types` — account addresses

To update Move version: change `rev` in all git deps to a new Sui commit/tag. Cargo resolves transitive workspace deps automatically.

### Research Insights — Dependency Strategy

> **Why git deps instead of vendoring?** (user preference for easy version updates)
> - Updating Move version = changing one `rev` value in `Cargo.toml`
> - No external code copied into the repo
> - Cargo caches git checkouts — no re-download on subsequent builds
> - CI works out of the box — no shallow-clone or vendor scripts needed
> - Tradeoff: first build is slow (~5-10 min) due to compiling full move-compiler dep tree, but subsequent builds use Cargo cache

**Third-party crates:**
- `clap` — CLI argument parsing
- `anyhow` — error handling
- `convert_case` — snake_case to camelCase conversion
- `insta` — snapshot testing for generated output

### Key Design Decisions

All decisions carried forward from brainstorm (see origin doc):

1. **Parser:** `move-compiler` crate, vendored, parser-level API only (`parse_file_string`)
2. **Input:** Single `.move` files or package directories (with `Move.toml`, recursive scan of `sources/`)
3. **Output:** One `.ts` file per module + shared `move2ts-errors.ts`, written to output dir
4. **Function signatures:** 2 args — `tx: Transaction` and typed args object
5. **Return values:** ALL wrappers return `TransactionResult` — the SDK always returns it from `moveCall` regardless of entry/public
6. **Singleton detection:** A struct is a singleton if it is constructed in `init()` AND no other function in the module constructs it (only `init()` can create it — doesn't matter if shared/transferred/frozen); singleton params become optional, backed by env vars (`{PROJECT}_{STRUCT}_ID` pattern, e.g., `MY_DEX_MARKETPLACE_ID`, `MY_DEX_ADMIN_CAP_ID`). In single-file mode, use `{MODULE}_{STRUCT}_ID`. Override via `--singletons`.
7. **Constants in generated TS:** camelCase (not UPPER_SNAKE_CASE)
8. **Object vs pure distinction:** `&T`/`&mut T` → `tx.object()` (object ref); value primitives/known stdlib types → `tx.pure.*()`. Unknown value types → emit warning and skip function
9. **Clock:** Auto-stripped from TS signature, auto-injected as `tx.object.clock()` in moveCall arguments
10. **Random:** Auto-stripped from TS signature, auto-injected as `tx.object.random()` in moveCall arguments
11. **TxContext:** Auto-stripped entirely (implicit in transactions)
11. **`public(package)` / private functions:** Skipped (not callable via external moveCall)
12. **`public entry` (if encountered):** Treated as `entry` (more restrictive constraint wins)
13. **Generic type params:** Named string params per type variable (`typeX: string, typeY: string`), passed via `typeArguments`
14. **CLI filtering:** `--methods` / `--skip-methods` use Move source names (snake_case). Mutually exclusive — error if both provided
15. **Generated file names:** snake_case matching Move module name (e.g., `marketplace.ts`)
16. **Parse errors:** Print diagnostics, collect ALL errors across files, report together, exit non-zero
17. **No functions found:** Skip module, print warning
18. **Overwrite behavior:** Always overwrite existing files. No cleanup of stale files
19. **Object arg types:** Use SDK's `TransactionObjectInput` type for all object params (both entry and public)
20. **Package ID validation:** Deferred to function call time (not module load time) — prevents import-time crashes in tests
21. **Env var validation:** Validate Sui address format (`/^0x[0-9a-fA-F]{1,64}$/`) before use
22. **Package ID env var naming:** In package mode (Move.toml), derive from project name: `{PROJECT}_PACKAGE_ID` (e.g., project `my_dex` → `MY_DEX_PACKAGE_ID`). In single-file mode, derive from module name: `{MODULE}_PACKAGE_ID`. Override with `--package-id-name MY_PACKAGE_ID` CLI flag.

### Research Insight — Entry vs Public Object Args

> The TS reviewer found that **entry functions CAN receive `TransactionResult` in PTBs**. The entry/public distinction is Move-level (can't call from other Move code), not PTB-level. In a PTB, you can `splitCoins` and pass the result to an entry function. The SDK's `TransactionObjectInput` already handles both `string` and `TransactionObjectArgument`. Using it for all functions simplifies generated code and is SDK-correct.

## Technical Approach

### Simplified File Structure

```
src/main.rs       -- entry point, error display, exit code
src/cli.rs        -- clap argument definitions
src/driver.rs     -- pipeline orchestration (parse → analyze → generate)
src/parser.rs     -- move-compiler wrapper, single CompilationEnv reuse
src/analyzer.rs   -- AST extraction + IR types + singleton detection
src/codegen.rs    -- TS rendering + CodeWriter + type mapping
vendor/           -- vendored move crates (stripped of bytecode/verifier deps)
tests/fixtures/   -- .move test files
tests/snapshots/  -- insta snapshot files for generated TS
```

### Research Insight — File Organization

> The simplicity reviewer and architecture strategist both found the original 10+ file structure premature. Start with 4 core source files. If any exceeds 500 lines during implementation, split with actual knowledge of where seams fall. The codegen split (5 files) and separate `types.rs`/`typemap.rs` add overhead without benefit at this scale.

### Phase 1: Project Scaffolding, Parser & Analyzer

**Files:** `Cargo.toml`, `src/main.rs`, `src/cli.rs`, `src/driver.rs`, `src/parser.rs`, `src/analyzer.rs`

#### CLI

```
move2ts <input> [options]

Arguments:
  <input>                          .move file or package directory (with Move.toml)

Options:
  -o, --output <dir>               Output directory (default: ./generated)
  --methods <method1,method2>      Generate only these methods (snake_case)
  --skip-methods <m1,m2>           Skip these methods (snake_case)
  --singletons <Struct=ENV_VAR>    Manual singleton overrides (escape hatch)
  --package-id-name <ENV_VAR>      Override package ID env var name (default: {PROJECT}_PACKAGE_ID)
```

Validation:
- Input path exists (file or directory)
- If directory, contains `Move.toml`
- `--methods` and `--skip-methods` are mutually exclusive
- Create output directory if it doesn't exist

#### Parser — Single CompilationEnv

```rust
pub struct MoveParser {
    env: CompilationEnv,  // created once, reused for all files
}

impl MoveParser {
    pub fn new() -> Self {
        let env = CompilationEnv::new(
            Flags::empty(), vec![], vec![], None,
            BTreeMap::new(),
            Some(PackageConfig {
                edition: Edition::E2024_BETA,
                ..Default::default()
            }),
            None,
        );
        Self { env }
    }

    pub fn parse_file(&self, source: &str) -> Result<Vec<Definition>> {
        let file_hash = FileHash::new(source);
        parse_file_string(&self.env, file_hash, source, None)
            .map_err(|diags| /* format diagnostics */)
    }
}
```

#### MoveType — Recursive Enum (Central IR Type)

```rust
pub enum MoveType {
    U8, U16, U32, U64, U128, U256,
    Bool,
    Address,
    SuiString,     // 0x1::string::String
    ObjectId,      // 0x2::object::ID
    Vector(Box<MoveType>),
    Option(Box<MoveType>),
    Ref { inner: Box<MoveType>, is_mut: bool },
    TypeParam(String),
    Struct { module: Option<String>, name: String, type_args: Vec<MoveType> },
    Unit,
}

impl MoveType {
    fn to_ts_type(&self) -> String { /* recursive match */ }
    fn to_tx_encoding(&self, expr: &str) -> String { /* tx.pure.* or tx.object() */ }
    fn is_object_ref(&self) -> bool { matches!(self, MoveType::Ref { .. }) }
}
```

#### IR Types

```rust
pub struct ModuleInfo {
    pub name: String,
    pub functions: Vec<FunctionInfo>,
    pub structs: Vec<StructInfo>,
    pub singletons: HashSet<String>,     // struct names only constructable in init()
}

pub struct FunctionInfo {
    pub name: String,                    // snake_case from source
    pub is_entry: bool,                  // entry modifier present
    pub type_params: Vec<String>,        // generic param names
    pub params: Vec<ParamInfo>,          // after stripping TxContext/Clock
    pub has_clock_param: bool,           // auto-inject tx.object.clock()
    pub has_random_param: bool,          // auto-inject tx.object.random()
    // Note: all wrappers return TransactionResult — no return_type field needed
}

pub struct ParamInfo {
    pub name: String,
    pub move_type: MoveType,
    pub is_singleton: bool,
}

pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, MoveType)>,
    pub has_key: bool,                   // distinguishes objects from value types
}
```

#### Singleton Detection

Algorithm (two-pass for borrow checker safety):

1. **First pass:** Scan ALL functions for struct constructor expressions. Build `HashMap<String, HashSet<String>>` mapping `struct_name → {constructing_function_names}`
2. **Second pass:** Extract function info, cross-referencing the singleton set to mark params

A struct is a singleton if:
- It appears in the constructor map
- The ONLY function that constructs it is `init`

Known limitations (documented):
- Helper functions called from `init()` that internally construct structs are NOT detected
- `--singletons Struct=ENV_VAR` CLI override is the escape hatch

### Phase 2: Type Mapping & Code Generator

**Files:** `src/codegen.rs`

#### Complete Type Mapping Table

| Move Type | TS Type | tx argument encoding |
|-----------|---------|---------------------|
| `u8` | `number` | `tx.pure.u8(v)` |
| `u16` | `number` | `tx.pure.u16(v)` |
| `u32` | `number` | `tx.pure.u32(v)` |
| `u64` | `bigint` | `tx.pure.u64(v)` |
| `u128` | `bigint` | `tx.pure.u128(v)` |
| `u256` | `bigint` | `tx.pure.u256(v)` |
| `bool` | `boolean` | `tx.pure.bool(v)` |
| `address` | `string` | `tx.pure.address(v)` |
| `0x1::string::String` | `string` | `tx.pure.string(v)` |
| `0x2::object::ID` | `string` | `tx.pure.id(v)` |
| `vector<u8>` | `Uint8Array` | `tx.pure('vector<u8>', v)` |
| `vector<T>` | `MappedT[]` | `tx.pure.vector('innerType', v)` |
| `Option<T>` | `MappedT \| null` | `tx.pure.option('innerType', v)` |
| `Coin<T>` / `Balance<T>` (by ref) | `TransactionObjectInput` | `tx.object(v)` |
| `&T` / `&mut T` (object) | `TransactionObjectInput` | `tx.object(v)` |

**Note:** `Option<T>` maps to `T | null` (not `undefined`) because the SDK's `tx.pure.option()` uses `null` for absent values.

**Vector and Option handling — recursive, depth-guarded:**

```rust
const MAX_TYPE_DEPTH: usize = 32;

fn map_type(ty: &MoveType, depth: usize) -> Result<TsTypeInfo> {
    if depth > MAX_TYPE_DEPTH {
        return Err(anyhow!("Type nesting too deep"));
    }
    match ty {
        MoveType::Vector(inner) if matches!(**inner, MoveType::U8) => Ok(uint8array()),
        MoveType::Vector(inner) => Ok(array(map_type(inner, depth + 1)?)),
        MoveType::Option(inner) => Ok(nullable(map_type(inner, depth + 1)?)),
        // ...
    }
}
```

#### CodeWriter Abstraction

```rust
pub struct CodeWriter {
    buffer: String,
    indent: usize,
}

impl CodeWriter {
    pub fn new() -> Self {
        Self { buffer: String::with_capacity(16 * 1024), indent: 0 }
    }
    pub fn line(&mut self, content: &str) { /* indent + content + \n */ }
    pub fn indent(&mut self) { self.indent += 1; }
    pub fn dedent(&mut self) { self.indent -= 1; }
    pub fn blank(&mut self) { self.buffer.push('\n'); }
    pub fn into_string(self) -> String { self.buffer }
}
```

#### Generated TS Output (Corrected)

```typescript
import process from 'node:process';
import type { TransactionObjectInput, TransactionResult } from '@mysten/sui/transactions';
import { Transaction } from '@mysten/sui/transactions';
import { Move2TsConfigError, validateSuiAddress } from './move2ts-errors';

// Package ID env var: derived from Move.toml project name, or overridden via --package-id-name
function getPackageId(): string {
  const id = process.env.MY_PROJECT_PACKAGE_ID;
  if (!id) {
    throw new Move2TsConfigError('MY_PROJECT_PACKAGE_ID environment variable is not set');
  }
  return validateSuiAddress(id, 'MY_PROJECT_PACKAGE_ID');
}

function getMarketplaceId(): string {
  const id = process.env.MY_PROJECT_MARKETPLACE_ID;
  if (!id) {
    throw new Move2TsConfigError('MY_PROJECT_MARKETPLACE_ID environment variable is not set');
  }
  return validateSuiAddress(id, 'MY_PROJECT_MARKETPLACE_ID');
}

export interface Listing {
  id: string;
  price: bigint;
  seller: string;
}

// Entry function — singleton resolved lazily
export function listItem(
  tx: Transaction,
  args: {
    price: bigint;
    marketplaceId?: TransactionObjectInput;
  },
): TransactionResult {
  return tx.moveCall({
    target: `${getPackageId()}::marketplace::list_item`,
    arguments: [
      tx.object(args.marketplaceId ?? getMarketplaceId()),
      tx.pure.u64(args.price),
    ],
  });
}

// Public function with generics
export function withdraw(
  tx: Transaction,
  args: {
    typeT: string;
    poolId: TransactionObjectInput;
    amount: bigint;
  },
): TransactionResult {
  return tx.moveCall({
    target: `${getPackageId()}::marketplace::withdraw`,
    typeArguments: [args.typeT],
    arguments: [
      tx.object(args.poolId),
      tx.pure.u64(args.amount),
    ],
  });
}

// Function with Clock — auto-injected
export function getTimedPrice(
  tx: Transaction,
  args: {
    marketplaceId?: TransactionObjectInput;
  },
): TransactionResult {
  return tx.moveCall({
    target: `${getPackageId()}::marketplace::get_timed_price`,
    arguments: [
      tx.object(args.marketplaceId ?? getMarketplaceId()),
      tx.object.clock(),
    ],
  });
}
```

### Research Insights — Generated Code Improvements

> **Why lazy getters instead of module-level const?** (from TS reviewer + pattern analyst)
> - Module-level `throw` breaks testing (can't import without setting all env vars)
> - Breaks tree-shaking
> - Breaks conditional code paths
> - Using getter functions defers validation to call time — consistent for both package ID and singletons
>
> **Why `TransactionObjectInput` instead of `string | TransactionResult`?**
> - It's the SDK's own type — broader compatibility (also accepts `CallArg`)
> - No need for `typeof` checks — just pass to `tx.object()` which accepts `TransactionObjectInput` directly
> - Cleaner generated code: `tx.object(args.poolId)` instead of `typeof args.poolId === 'string' ? tx.object(args.poolId) : args.poolId`
>
> **Why return `TransactionResult` from all functions?**
> - The SDK always returns it from `tx.moveCall()` regardless of entry/public
> - Discarding it limits composability (e.g., `const [item] = listItem(tx, {...})`)
> - Entry functions in Move return nothing, but `TransactionResult` is a transaction-graph handle, not a Move return value

#### `move2ts-errors.ts`

```typescript
export class Move2TsConfigError extends Error {
  override readonly name = 'Move2TsConfigError' as const;
  constructor(message: string) {
    super(message);
  }
}

export function validateSuiAddress(value: string, name: string): string {
  if (!/^0x[0-9a-fA-F]{1,64}$/.test(value)) {
    throw new Move2TsConfigError(`${name} is not a valid Sui address: ${value}`);
  }
  return value;
}
```

### Phase 3: Testing

- **Snapshot tests** (`insta` crate) — given a `.move` fixture file, run full pipeline, snapshot the `.ts` output. Any codegen change shows as a snapshot diff.
- **Integration tests** — verify generated TS compiles with `tsc --strict`
- **Unit tests** — type mapping, singleton detection, name conversion

### Phase 4: CI/CD & npm Publishing

**Files:** `.github/workflows/release.yml`, `npm/` directory

#### CI Fixes from Research

> **Pin GitHub Actions to SHA hashes** (security sentinel) — prevents supply chain attacks via tag re-pointing:
> ```yaml
> uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11 # v4.1.1
> ```
>
> **Fix macOS x86_64 build** (deployment agent) — `macos-latest` is now arm64. Use `macos-13` for x86_64.
>
> **Fix aarch64-linux** (deployment agent) — plain `cargo build` won't work. Install `gcc-aarch64-linux-gnu` and configure linker in `.cargo/config.toml`, or use `cross` with vendored crates inside project directory.
>
> **Vendored crates eliminate Sui clone entirely** — no more 5-15 min shallow clones per build.

#### Release Profile

```toml
[profile.release]
lto = true
opt-level = 's'
strip = true
codegen-units = 1
```

Expected binary size: 3-8MB (with vendored/stripped parser-only deps).

#### npm Publish Pipeline (Complete)

Platform packages MUST include `os`/`cpu` fields and be published BEFORE the main package:

```yaml
name: Release
permissions:
  contents: write
  id-token: write  # for npm provenance

on:
  push:
    tags: ['v*']

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            npm_pkg: linux-x64
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            npm_pkg: linux-arm64
          - target: x86_64-apple-darwin
            os: macos-13           # Intel runner
            npm_pkg: darwin-x64
          - target: aarch64-apple-darwin
            os: macos-latest       # Apple Silicon runner
            npm_pkg: darwin-arm64
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            npm_pkg: win32-x64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@b4ffde65  # pinned SHA
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: move2ts-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/move2ts*

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@b4ffde65
      - uses: actions/download-artifact@v4
      - name: Generate checksums
        run: sha256sum move2ts-*/move2ts* > checksums-sha256.txt
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            move2ts-*/move2ts*
            checksums-sha256.txt
          generate_release_notes: true

  publish-npm:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@b4ffde65
      - uses: actions/download-artifact@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 10
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          registry-url: https://registry.npmjs.org
      - name: Stamp version from git tag
        run: |
          VERSION="${GITHUB_REF_NAME#v}"
          for pkg in npm/*/package.json; do
            jq --arg v "$VERSION" '.version = $v' "$pkg" > tmp && mv tmp "$pkg"
          done
      - name: Assemble platform binaries
        run: |
          chmod +x move2ts-*/move2ts 2>/dev/null || true
          cp move2ts-x86_64-unknown-linux-gnu/move2ts npm/linux-x64/
          cp move2ts-aarch64-unknown-linux-gnu/move2ts npm/linux-arm64/
          cp move2ts-x86_64-apple-darwin/move2ts npm/darwin-x64/
          cp move2ts-aarch64-apple-darwin/move2ts npm/darwin-arm64/
          cp move2ts-x86_64-pc-windows-msvc/move2ts.exe npm/win32-x64/
      - name: Publish platform packages then main package
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
        run: |
          for pkg in linux-x64 linux-arm64 darwin-x64 darwin-arm64 win32-x64; do
            cd npm/$pkg && pnpm publish --access public --no-git-checks --provenance && cd ../..
          done
          cd npm/move2ts && pnpm publish --access public --no-git-checks --provenance
```

Each platform `package.json` must include:
```json
{
  "name": "@move2ts/linux-x64",
  "os": ["linux"],
  "cpu": ["x64"],
  "publishConfig": { "access": "public" }
}
```

## Acceptance Criteria

### Functional Requirements

- [ ] Parse single `.move` files and generate corresponding `.ts` files
- [ ] Parse Move package directories (recursive `sources/` scan)
- [ ] Generate type-safe function wrappers — all return `TransactionResult`
- [ ] Object params use `TransactionObjectInput` (SDK type)
- [ ] Generate TypeScript interfaces for Move structs referenced in function signatures
- [ ] Detect singletons (structs only constructed in `init()`) — optional params backed by env vars
- [ ] `--singletons` CLI override for manual singleton specification
- [ ] Handle generic type parameters as named string args
- [ ] Auto-strip `TxContext` params; auto-inject `Clock` as `tx.object.clock()` and `Random` as `tx.object.random()`
- [ ] `--methods` and `--skip-methods` filtering with validation
- [ ] Generate shared `move2ts-errors.ts` with `Move2TsConfigError` and `validateSuiAddress`
- [ ] Correct recursive type mapping for vectors, options, and nested combinations
- [ ] `vector<u8>` special-cased to `Uint8Array`
- [ ] camelCase for all generated TS constants
- [ ] Lazy env var validation (at call time, not import time)
- [ ] Validate Sui address format for env var values
- [ ] Collect all parse errors across files before exiting
- [ ] `--package-id-name` CLI override for package ID env var name
- [ ] Package ID env var derived from Move.toml project name (package mode) or module name (single-file mode)
- [ ] Only generate interfaces for structs referenced in function parameters (not all structs)

### Non-Functional Requirements

- [ ] Generated code compiles with `tsc --strict`
- [ ] Generated code is ESM-only, portable across Node.js, Deno, Bun
- [ ] CLI exits non-zero on parse errors with readable diagnostics
- [ ] Snapshot tests for all codegen output (`insta`)
- [ ] GitHub Actions builds binaries for 5 targets with SHA-pinned actions
- [ ] npm packages published with provenance attestation
- [ ] Release binaries include SHA-256 checksums
- [ ] Release profile: `lto = true`, `strip = true`, `codegen-units = 1`

## Implementation Phases

### Phase 1: Scaffolding + Parser + Analyzer
- Vendor move crates into `vendor/`
- `Cargo.toml` with vendored path deps
- CLI with clap (including `--singletons` escape hatch)
- `MoveParser` with single `CompilationEnv` reuse
- `MoveType` recursive enum
- IR types (`ModuleInfo`, `FunctionInfo`, etc.)
- AST extraction + singleton detection
- **Test:** Parse a real `.move` file, extract functions, verify IR matches expected

### Phase 2: Code Generator
- `CodeWriter` abstraction
- TS file renderer (lazy getters, `TransactionObjectInput`, functions, interfaces, imports)
- `move2ts-errors.ts` generator (with `validateSuiAddress`)
- Clock auto-injection
- Generic type arguments
- **Test:** Snapshot tests — generate TS from fixtures, verify with `insta`

### Phase 3: Testing & Validation
- Integration test: generated TS compiles with `tsc --strict`
- Edge case fixtures: nested generics, multiple singletons, no functions, multiple modules per file
- **Test:** Full pipeline end-to-end on real Move packages from `~/Projects/byz-sui-move-modules/`

### Phase 4: CI/CD & npm Publishing
- GitHub Actions release workflow (SHA-pinned)
- Cross-platform build matrix (fixed macOS/Linux targets)
- npm package structure with `os`/`cpu` fields
- Version stamping from git tag
- Binary assembly + `chmod +x`
- Provenance attestation
- SHA-256 checksums
- **Test:** Tag a release, verify all 5 platform binaries + npm packages published

## Sources & References

### Origin

- **Brainstorm document:** [docs/brainstorms/2026-03-21-move2ts-brainstorm.md](docs/brainstorms/2026-03-21-move2ts-brainstorm.md)

### Internal References

- Move compiler parser API: `~/Projects/sui/external-crates/move/crates/move-compiler/src/parser/syntax.rs:4945`
- Parser AST types: `~/Projects/sui/external-crates/move/crates/move-compiler/src/parser/ast.rs`
- Sui mode constants: `~/Projects/sui/external-crates/move/crates/move-compiler/src/sui_mode/mod.rs`
- Move-analyzer parsing pattern: `~/Projects/sui/external-crates/move/crates/move-analyzer/src/symbols/mod_extensions.rs:158-173`
- Existing TS SDK patterns: `~/Projects/sui-trading-sdk/`, `~/Projects/sui-defi-sdk/`

### SDK API References

- `Transaction` class: `@mysten/sui/transactions`
- `TransactionObjectInput`: `string | TransactionObjectArgument` — accepts both object IDs and prior command results
- `TransactionResult`: always returned from `tx.moveCall()` — intersection of `Result` and `NestedResult[]`
- `tx.pure.*` methods: `u8`, `u16`, `u32`, `u64`, `u128`, `u256`, `bool`, `address`, `string`, `id`, `vector(type, values)`, `option(type, value)`
- `tx.object.clock()`: shared Clock object (`0x6`)
- `tx.object.random()`: shared Random object (`0x8`)
