---
title: Correct TypeScript type resolution for Move struct and generic params
category: logic-errors
date: 2026-03-21
tags: [codegen, type-mapping, TransactionObjectInput, generic, struct, sui]
module: codegen
symptom: "Generated TS uses raw struct names (Coin, Kiosk) or tx.pure() instead of TransactionObjectInput/tx.object()"
root_cause: "Type resolution lacked context about whether a struct is a local pure value or an external object, and generic type params didn't carry ability constraints"
---

## Problem

Three related type resolution bugs in generated TypeScript:

1. **Generic `T: key + store` params** — `nft: T` generated `tx.pure(args.nft)` instead of `tx.object(args.nft)`. The `T` is an object (has `key`), not a pure value.

2. **External object structs** — `coin: Coin<USDC>` generated `coin: Coin` as the TS type instead of `coin: TransactionObjectInput`. `Coin` is a Sui framework object, not a type we define.

3. **Ref structs getting interfaces** — `store: &mut Store` caused `export interface Store { ... }` to be generated, but `Store` is passed as `TransactionObjectInput` (object ID), so the interface is useless.

## Root Cause

### Bug 1: TypeParam had no constraint info
`MoveType::TypeParam` was `TypeParam(String)` — just the name, no abilities. The codegen defaulted to `tx.pure()` for all type params.

### Bug 2: to_ts_type lacked module context
`to_ts_type` for `MoveType::Struct` returned the raw struct name. It couldn't distinguish between the module's own pure value structs (which need their interface name for BCS) and external object structs (which need `TransactionObjectInput`).

### Bug 3: collect_referenced_structs followed Ref types
The function recursed into `Ref { inner }`, finding structs inside `&T`/`&mut T`. But ref types always map to `TransactionObjectInput` — the inner struct is irrelevant for interface generation.

## Solution

### Fix 1: Track has_key on TypeParam

```rust
// Before
TypeParam(String)

// After
TypeParam { name: String, has_key: bool }
```

In codegen:
```rust
MoveType::TypeParam { has_key, .. } => {
    if *has_key { "TransactionObjectInput" } else { "string" }
}
```

### Fix 2: Context-aware type resolution for params

```rust
fn to_ts_type_for_param(ty: &MoveType, own_pure_structs: &HashSet<String>) -> String {
    match ty {
        MoveType::Struct { name, .. } => {
            if own_pure_structs.contains(name.as_str()) {
                name.clone()  // module's own copy+drop struct → use interface
            } else {
                "TransactionObjectInput".to_string()  // external → object
            }
        }
        _ => to_ts_type(ty),
    }
}
```

### Fix 3: Don't follow Ref in collect_referenced_structs

```rust
MoveType::Ref { .. } => {
    // Ref maps to TransactionObjectInput, not the struct interface. Skip.
}
```

## Key Rule

When generating TypeScript param types for Move function wrappers:

| Move param type | TS type | Encoding |
|---|---|---|
| `&T` / `&mut T` | `TransactionObjectInput` | `tx.object()` |
| `T` where `T: key` | `TransactionObjectInput` | `tx.object()` |
| `Coin<X>`, `Kiosk`, etc. (external) | `TransactionObjectInput` | `tx.object()` |
| Module's own copy+drop struct | Interface name | `tx.pure(bcs.struct(...))` |
| Primitives | Mapped type | `tx.pure.*()` |

**Default to `TransactionObjectInput` for any struct not in the module's own pure value set.**

## Prevention

- When adding new type mapping logic, always test with real-world Move contracts (not just toy fixtures)
- Test with external framework types: `Coin<T>`, `Kiosk`, `KioskOwnerCap`, `PurchaseCap<T>`, `TransferPolicy<T>`
- Test generic params with `key + store` constraints
- The Tradeport listings contract is a good integration test — it uses all these patterns
