---
title: "feat: Add VecMap<K, V> type support"
type: feat
status: active
date: 2026-03-22
origin: docs/brainstorms/2026-03-22-vecmap-support-requirements.md
---

# feat: Add VecMap<K, V> Type Support

## Overview

Add first-class support for Sui Move's `VecMap<K, V>` type throughout the move2ts pipeline: analyzer recognition, IR representation, TypeScript type mapping, and BCS serialization. VecMap is internally `vector<Entry<K, V>>` but the `@mysten/bcs` SDK provides `bcs.map(keyType, valueType)` which handles the wire format transparently.

## Problem Statement / Motivation

Move code using `sui::vec_map::VecMap<K, V>` currently falls through to `MoveType::Struct { name: "VecMap" }`, which codegen treats as `TransactionObjectInput`. This is wrong — VecMap is a pure value type (copy+drop+store) that should map to `Map<K, V>` in TypeScript and be passed via BCS serialization. (see origin: docs/brainstorms/2026-03-22-vecmap-support-requirements.md)

## Proposed Solution

Add `MoveType::VecMap(Box<MoveType>, Box<MoveType>)` as a first-class IR variant, following the same pattern as `Vector` and `Option`. Recognize it in the analyzer, map it in all codegen paths. (see origin: key decision — first-class IR variant)

### Encoding Strategy for Nested VecMap

There is no `tx.pure.map()` SDK helper and no BCS type string for maps. The current `to_tx_encoding` for `Vector<inner>` uses `tx.pure.vector('bcs_type_string', expr)` — this approach cannot express maps.

**Decision**: Use the explicit BCS schema path conditionally:
- Standalone VecMap param: `tx.pure(bcs.map(K_schema, V_schema).serialize(expr))`
- `Vector<VecMap<K, V>>`: `tx.pure(bcs.vector(bcs.map(K_schema, V_schema)).serialize(expr))`
- Simple `Vector<u64>`: keep existing `tx.pure.vector('u64', expr)` (no output churn)

This requires a helper to detect whether a type transitively contains VecMap, so `to_tx_encoding` can switch strategies for Vector/Option that wrap VecMap.

## Technical Considerations

### `to_bcs_type_string` panic risk

This function panics on unsupported types. `Vector<VecMap<...>>` would call `to_bcs_type_string` on VecMap and crash the CLI. The conditional encoding strategy above avoids this path entirely — when a Vector/Option contains VecMap, we bypass `to_bcs_type_string` and use `to_bcs_schema` instead.

### `needs_bcs` / `needs_address` import detection

Currently these only check `MoveType::Struct` params against `used_pure_structs`. A standalone `VecMap<K, V>` param is not a Struct, so both flags would be false — missing imports. Must extend to recursively scan all parameter types for VecMap (triggers `needs_bcs`) and Address/ObjectId within VecMap key/value (triggers `needs_address`).

### `collect_struct_refs_from_type`

Must recurse into VecMap key/value types. Without this, `VecMap<String, MyPureStruct>` would not emit the `MyPureStruct` interface.

## Acceptance Criteria

### Phase 1: IR + Analyzer

- [ ] Add `VecMap(Box<MoveType>, Box<MoveType>)` variant to `MoveType` enum in `src/ir.rs`
- [ ] Recognize `VecMap` in `convert_single_name` (unqualified) in `src/analyzer.rs`
- [ ] Recognize `VecMap` in `convert_apply_type` Path branch (qualified: `vec_map::VecMap`, `0x2::vec_map::VecMap`, `sui::vec_map::VecMap`) in `src/analyzer.rs`
- [ ] Unit tests: parse VecMap from both unqualified and qualified paths, nested types

### Phase 2: Codegen — Type Mapping

- [ ] `to_ts_type`: VecMap(K, V) → `Map<K_ts, V_ts>` in `src/codegen.rs`
- [ ] `to_ts_type_for_param`: fall through to `to_ts_type` (no special handling needed)
- [ ] `generate_struct_interface`: works automatically via `to_ts_type`
- [ ] `generate_event_types`: no change needed (all fields are `string`)
- [ ] Unit tests: TS type mapping for VecMap with various key/value types

### Phase 3: Codegen — BCS Encoding

- [ ] `to_bcs_schema`: VecMap(K, V) → `bcs.map(K_schema, V_schema)` in `src/codegen.rs`
- [ ] `to_tx_encoding`: VecMap(K, V) → `tx.pure(bcs.map(K_schema, V_schema).serialize(expr))`
- [ ] Add `type_contains_vecmap` helper to detect VecMap anywhere in a type tree
- [ ] `to_tx_encoding`: Vector/Option arms — when inner type contains VecMap, switch to `tx.pure(bcs.vector/option(inner_schema).serialize(expr))` using `to_bcs_schema` instead of `to_bcs_type_string`
- [ ] `to_tx_encoding_with_context`: handle VecMap (not a struct, needs direct BCS encoding)
- [ ] Unit tests: BCS schema, tx encoding for standalone/nested VecMap

### Phase 4: Codegen — Import Detection + Helpers

- [ ] `type_contains_address`: recurse into VecMap key and value
- [ ] `collect_struct_refs_from_type`: recurse into VecMap key and value
- [ ] Extend `needs_bcs` computation: scan all param types recursively for VecMap (not just Struct params)
- [ ] Extend `needs_address` computation: scan VecMap key/value types for Address/ObjectId
- [ ] Unit tests: import detection with VecMap<address, u64>, VecMap<String, PureStruct>

### Phase 5: Integration Tests + Fixture

- [ ] Create `tests/fixtures/vecmap.move`: VecMap in function params (standalone, in struct field, nested in Vector/Option, in events, via qualified path)
- [ ] Add integration test `full_pipeline_vecmap` in `tests/integration_test.rs`
- [ ] Add VecMap output to `tests/ts-check/` and verify it compiles with `tsc`

## Implementation Reference

All locations requiring changes (Rust compiler will catch most via exhaustive matching, but import detection logic is NOT a match expression):

| File | Function/Item | Change |
|------|---------------|--------|
| `src/ir.rs:18` | `MoveType` enum | Add `VecMap(Box<MoveType>, Box<MoveType>)` |
| `src/analyzer.rs:219` | `convert_single_name` | Add `"VecMap"` arm taking two type args |
| `src/analyzer.rs:148` | `convert_apply_type` Path branch | Add `"VecMap"` match with module/root checks |
| `src/codegen.rs:58` | `to_ts_type` | Add `VecMap(k, v)` → `Map<K, V>` |
| `src/codegen.rs:112` | `to_tx_encoding` | Add VecMap arm + conditional Vector/Option handling |
| `src/codegen.rs:150` | `to_bcs_schema` | Add `VecMap(k, v)` → `bcs.map(K, V)` |
| `src/codegen.rs:202` | `to_tx_encoding_with_context` | Handle VecMap (not a struct) |
| `src/codegen.rs:220` | `type_contains_address` | Recurse into key and value |
| `src/codegen.rs:241` | `to_bcs_type_string` | No change needed (bypassed for VecMap) |
| `src/codegen.rs:297` | `needs_bcs` computation | Extend to detect VecMap in all params |
| `src/codegen.rs:312` | `needs_address` computation | Extend to recurse into VecMap |
| `src/codegen.rs:510` | `collect_struct_refs_from_type` | Recurse into key and value |

## Dependencies & Risks

- `@mysten/bcs` v1.9.2+ provides `bcs.map(keyType, valueType)` — already in the project's test dependencies (see origin: dependency assumption)
- Risk: `to_bcs_type_string` panic if conditional bypass is missed. Mitigated by adding a `VecMap => panic!("use to_bcs_schema for VecMap")` arm with a clear message.

## Scope Boundaries

- VecSet is out of scope (could follow same pattern later) (see origin)
- Table, Bag, and dynamic-field collections are out of scope (they are on-chain objects) (see origin)
- VecMap in return types is out of scope (functions return `TransactionResult`)

## Sources & References

- **Origin document:** [docs/brainstorms/2026-03-22-vecmap-support-requirements.md](docs/brainstorms/2026-03-22-vecmap-support-requirements.md) — Key decisions: first-class IR variant, use bcs.map() from SDK
- **Institutional learnings:** `docs/solutions/logic-errors/move-type-to-ts-type-resolution.md` — pattern for adding MoveType variants
- **Institutional learnings:** `docs/solutions/integration-issues/sui-sdk-transaction-types.md` — tx.pure.* encoding patterns
- Similar patterns: `src/ir.rs:18-19` (Vector/Option variants), `src/analyzer.rs:219-226` (Vector/Option recognition)
