---
title: Event type fields must use snake_case, not camelCase
category: logic-errors
date: 2026-03-21
tags: [events, codegen, naming, snake-case, sui-indexer]
module: codegen
symptom: "Generated event type fields used camelCase (itemId) but Sui RPC/indexer responses use snake_case (item_id)"
root_cause: "Event field generation reused to_camel_case() from function param rendering, but event data from indexers preserves Move's original snake_case names"
---

## Problem

Event types were generated with camelCase field names:

```typescript
export type ItemPurchased = {
  readonly buyer: string;
  readonly itemId: string;    // WRONG — indexer returns item_id
};
```

But Sui RPC responses (`suix_queryEvents`) return event fields in their original Move snake_case names. Using camelCase in the type definition would cause type mismatches when parsing indexer data.

## Root Cause

The event field rendering reused `to_camel_case()` — the same function used for function parameter names. But function params and event fields have different naming requirements:

- **Function params** → camelCase (TypeScript convention for function args)
- **Event fields** → snake_case (must match indexer/RPC response format)

## Solution

Use the original field name directly instead of converting:

```rust
// Before (wrong)
w.line(&format!("readonly {}: string;", to_camel_case(field_name)));

// After (correct)
w.line(&format!("readonly {field_name}: string;"));
```

## Prevention

- Event types represent **external data shapes** from the Sui indexer — field names must match the wire format
- Function params represent **TypeScript API** — field names follow TS conventions (camelCase)
- When adding new codegen outputs, always ask: "does this represent an internal API or external data?"
