---
title: "Fix BCS Address import, add VecMap support, and fix nested struct BCS expansion"
category: logic-errors
date: 2026-03-22
tags:
  - bcs
  - address
  - vecmap
  - codegen
  - typescript
  - nested-structs
  - type-mapping
  - sui-sdk
module: src/codegen.rs, src/ir.rs, src/analyzer.rs
severity: high
---

# BCS Type Mapping: Address Import, VecMap Support, and Nested Struct Expansion

## Problem

Three related codegen bugs caused incorrect TypeScript output for BCS-serialized types:

1. **`bcs.Address` referenced but never importable.** `to_bcs_schema()` emitted `bcs.Address` for `MoveType::Address` and `MoveType::ObjectId`, but the `bcs` object from `@mysten/bcs` does NOT have an `Address` property. `Address` is a Sui-specific BCS type available on the `bcs` re-export from `@mysten/sui/bcs`.

2. **`VecMap<K, V>` treated as unknown Struct.** `VecMap` from `sui::vec_map` fell through to `MoveType::Struct { name: "VecMap" }`, which codegen treated as `TransactionObjectInput` (passed via `tx.object()`). But VecMap is a copy+drop+store value type that should be BCS-serialized.

3. **Nested copy+drop structs not expanded in BCS schemas.** When a pure value struct contained another pure value struct as a field, the inner struct fell back to `bcs.bytes(32)` instead of being recursively expanded via `bcs.struct(...)`.

## Root Cause

1. **Address import**: `@mysten/sui@2.9+` changed the export structure. `Address` is no longer a standalone named export from `@mysten/sui/bcs` — it's a property on the `suiBcs` object re-exported as `bcs`. The correct usage is `import { bcs } from '@mysten/sui/bcs'` then `bcs.Address`.

2. **VecMap**: No `MoveType::VecMap` variant existed. The type fell through to the generic `Struct` handling.

3. **Nested structs**: `to_bcs_schema()` had no access to `&[StructInfo]`, so it couldn't look up nested structs to expand them.

## Solution

### Fix 1: Conditional BCS Import Source

Import `bcs` from `@mysten/sui/bcs` (which re-exports everything from `@mysten/bcs` plus Sui-specific types like `bcs.Address`) when Address/ObjectId types appear anywhere in BCS schemas:

```rust
if needs_bcs && needs_address {
    w.line("import { bcs } from '@mysten/sui/bcs';");
} else if needs_bcs {
    w.line("import { bcs } from '@mysten/bcs';");
}
```

Detection uses `type_contains_address()` which recursively walks the entire type tree including nested pure value structs and VecMap key/value types.

### Fix 2: First-Class VecMap Variant

Added `MoveType::VecMap(Box<MoveType>, Box<MoveType>)` to the IR enum, with recognition in both analyzer paths (unqualified `VecMap` and qualified `0x2::vec_map::VecMap`, `sui::vec_map::VecMap`, `vec_map::VecMap`).

Codegen mappings:
- **TypeScript type**: `Map<K_ts, V_ts>`
- **BCS schema**: `bcs.map(K_schema, V_schema)`
- **Transaction encoding**: `tx.pure(bcs.map(K_schema, V_schema).serialize(expr))`

Critical detail — `to_bcs_type_string` intentionally panics on VecMap because there is no BCS type string for maps (no `tx.pure.map()` helper exists). A `type_contains_vecmap()` guard on `Vector`/`Option` arms in `to_tx_encoding` switches to the explicit BCS schema path:

```rust
MoveType::Vector(inner) if type_contains_vecmap(inner) => {
    format!("tx.pure(bcs.vector({}).serialize({expr}))", to_bcs_schema(inner, &[]))
}
```

This avoids the `to_bcs_type_string` panic while preserving the existing `tx.pure.vector('u64', ...)` output for simple types.

### Fix 3: Thread `&[StructInfo]` Through BCS Functions

`to_bcs_schema`, `to_bcs_struct_schema`, `to_bcs_struct_encoding`, and `type_contains_address` all received a `structs: &[StructInfo]` parameter. When `to_bcs_schema` encounters a `MoveType::Struct`, it looks up the struct — if it's a pure value type, it recursively expands it.

## The Three-Tier Encoding Pattern

This is the governing architecture for how Move types are encoded in generated TypeScript:

| Tier | Encoding | Examples | Type String? |
|------|----------|----------|-------------|
| 1 | `tx.pure.<method>()` | u64, bool, address, vector<u64>, option<bool> | Yes — `to_bcs_type_string()` |
| 2 | `tx.pure(bcs.<schema>.serialize())` | VecMap, pure value structs | No — `to_bcs_schema()` |
| 3 | `tx.object()` | key structs, external object types | N/A |

When a **Tier 2** type appears inside `Vector` or `Option`, the encoding must detect this and switch from the Tier 1 path (`tx.pure.vector('type_string', ...)`) to the Tier 2 path (`tx.pure(bcs.vector(schema).serialize(...))`). This is what `type_contains_vecmap()` does.

## Prevention: Checklist for Adding New MoveType Variants

When adding a new variant (e.g., `VecSet`, `Table`), update ALL of the following:

**IR (`src/ir.rs`):** Add the variant to `enum MoveType`.

**Analyzer (`src/analyzer.rs`):**
- `convert_single_name()` — unqualified name
- `convert_apply_type()` — qualified path with module/root checks

**Codegen (`src/codegen.rs`) — every `match ty` block:**
- `to_ts_type()` — TypeScript type
- `to_tx_encoding()` + `to_tx_encoding_with_context()` — encoding call
- `to_bcs_schema()` — BCS schema builder
- `to_bcs_type_string()` — type string OR explicit panic for Tier 2 types
- `type_contains_address()` — recurse into inner types
- `type_contains_vecmap()` — recurse (or generalize to `type_needs_schema_encoding()`)
- `collect_struct_refs_from_type()` — recurse for interface generation

**Import detection in `generate_module()`:**
- `needs_bcs` — does this type require `bcs` import?
- `needs_address` — does encoding use `bcs.Address`?

**Tests:**
- Unit tests per codegen function
- Move fixture in `tests/fixtures/`
- Integration test `full_pipeline_*`
- **Add to `generated_ts_compiles_with_tsc`** — this is what catches wrong imports

The Rust compiler catches most missing match arms via exhaustive matching. But `needs_bcs`/`needs_address` computation is NOT a match expression — it's filter logic that must be manually extended.

## Related Documentation

- `docs/solutions/logic-errors/move-type-to-ts-type-resolution.md` — Pattern for extending MoveType variants (TypeParam was the precedent)
- `docs/solutions/integration-issues/sui-sdk-transaction-types.md` — tx.pure.* encoding patterns and SDK import paths
- `docs/solutions/logic-errors/event-field-naming-convention.md` — Event fields stay snake_case regardless of type
