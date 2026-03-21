---
title: Correct Sui SDK types for generated transaction wrappers
category: integration-issues
date: 2026-03-21
tags: [sui-sdk, typescript, TransactionObjectInput, TransactionResult, moveCall]
module: codegen
symptom: Generated TypeScript code uses wrong types for object params and return values
root_cause: Misunderstanding of Sui SDK type system — entry vs public distinction is Move-level, not PTB-level
---

## Problem

When generating TypeScript wrappers for Move functions, multiple type decisions seemed ambiguous:
- Should entry functions have different object param types than public functions?
- Should entry functions return `void` while public functions return `TransactionResult`?
- How to handle object params that could be either string IDs or results from prior tx commands?

## Solution

### All object params use `TransactionObjectInput`

The SDK type `TransactionObjectInput = string | TransactionObjectArgument` handles both string IDs and prior command results. Use it for ALL functions (entry AND public) because:

- **Entry functions CAN receive `TransactionResult` in PTBs** — the entry/public distinction is Move-level (can't call from other Move code), not PTB-level
- `tx.object()` accepts `TransactionObjectInput` directly — no `typeof` checks needed

```typescript
import type { TransactionObjectInput } from '@mysten/sui/transactions';

export function listItem(tx: Transaction, args: {
  marketplaceId?: TransactionObjectInput;  // not string!
  price: bigint;
}): TransactionResult { ... }
```

### All functions return `TransactionResult`

The SDK's `tx.moveCall()` always returns `TransactionResult` regardless of entry/public. Discarding it limits composability:

```typescript
const [item] = listItem(tx, { price: 100n }); // destructurable!
```

### Lazy env var validation

Validate package IDs at **call time**, not import time. Module-level throws break testing and tree-shaking:

```typescript
// BAD — throws on import
const packageId = process.env.MY_PACKAGE_ID;
if (!packageId) throw new Error('...');

// GOOD — throws only when function is called
function getPackageId(): string {
  const id = process.env.MY_PACKAGE_ID;
  if (!id) throw new InvalidConfigError('...');
  if (!isValidSuiAddress(id)) throw new InvalidConfigError('...');
  return id;
}
```

Use `isValidSuiAddress` from `@mysten/sui/utils` — don't roll your own.

## Prevention

- Check the SDK's own TypeScript types before designing generated code
- Test generated code compilation against real `@mysten/sui` types (not stubs)
- `tx.pure.*` methods: u8-u256 accept their respective types, u64/u128/u256 accept `string | number | bigint`
- `tx.object.clock()` and `tx.object.random()` for shared system objects
