---
date: 2026-03-22
topic: vecmap-support
---

# VecMap<K, V> Support

## Problem Frame

Move code using `sui::vec_map::VecMap<K, V>` currently falls through to `MoveType::Struct { name: "VecMap" }` in the analyzer, which codegen treats as `TransactionObjectInput` (object reference). This is wrong — VecMap is a pure value type (copy+drop+store) that should be passed via BCS serialization, not `tx.object()`. Internally it is `vector<Entry<K, V>>` where `Entry` has `key: K` and `value: V` fields.

## Requirements

- R1. Recognize `VecMap<K, V>` from `0x2::vec_map` (and `sui::vec_map`) in the analyzer and produce a first-class `MoveType::VecMap(K, V)` IR variant, consistent with how `Vector` and `Option` are handled.
- R2. Map `VecMap<K, V>` to TypeScript type `Map<K_ts, V_ts>` in interfaces and function parameter types.
- R3. Encode `VecMap<K, V>` as `bcs.map(K_schema, V_schema)` in BCS struct schemas — the `@mysten/bcs` SDK already provides `bcs.map()`.
- R4. Encode standalone `VecMap<K, V>` function parameters via `tx.pure` using the correct BCS map serialization.
- R5. Support nesting: `Vector<VecMap<K, V>>`, `Option<VecMap<K, V>>`, and VecMap as a field inside pure value structs should all work.
- R6. Event fields with VecMap type continue to use `string` (no change — events stringify all fields).

## Success Criteria

- A Move function with a `VecMap<u64, bool>` param generates a TS wrapper accepting `Map<bigint, boolean>` and serializes it correctly via `bcs.map(bcs.u64(), bcs.bool())`.
- Nested types like `Option<VecMap<address, u64>>` produce `Map<string, bigint> | null` in TS and `bcs.option(bcs.map(Address, bcs.u64()))` in BCS.
- All existing tests continue to pass.

## Scope Boundaries

- VecSet is out of scope for this change (could follow the same pattern later).
- Table, Bag, and other dynamic-field collections are out of scope (they are on-chain objects, not pure values).

## Key Decisions

- **First-class IR variant**: `MoveType::VecMap(Box<MoveType>, Box<MoveType>)` rather than special-casing Struct in codegen. Rationale: consistent with Vector/Option pattern, every codegen path handles it uniformly.
- **Use `bcs.map()`**: The SDK already provides this — no need to manually construct `bcs.vector(bcs.struct('Entry', { key, value }))`.

## Dependencies / Assumptions

- `@mysten/bcs` v1.9.2+ provides `bcs.map(keyType, valueType)`.

## Outstanding Questions

### Deferred to Planning

- [Affects R4][Technical] What is the correct `tx.pure.*` call for a standalone VecMap param? There may not be a `tx.pure.vecMap()` helper — may need `tx.pure(bcs.map(...).serialize(...))`.
- [Affects R2][Technical] How should `to_bcs_type_string` represent VecMap for `tx.pure.vector('...')` nesting? Likely not needed since VecMap params would use `bcs.map()` directly.

## Next Steps

-> `/ce:plan` for structured implementation planning
