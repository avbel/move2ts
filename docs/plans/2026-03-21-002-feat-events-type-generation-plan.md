---
title: "feat: Generate TypeScript types for Move events"
type: feat
status: active
date: 2026-03-21
---

# feat: Generate TypeScript types for Move events

## Overview

Add a `--events` CLI flag that generates TypeScript type aliases for Move event structs. Events in Sui Move are structs with `copy` and `drop` abilities that are emitted via `sui::event::emit()`. The generated TS types have all fields typed as `string` (no Move type mapping) since event data comes from indexer/RPC responses where all values are serialized as strings.

## Proposed Solution

### CLI Change

Add `--events` boolean flag:

```
move2ts <input> [options]

Options:
  --events                         Include event type definitions in output
```

When `--events` is present, generate `export type` declarations for event structs.

### Event Detection

A struct is an event candidate if it has **both `copy` and `drop`** abilities. However, copy+drop structs that appear as function parameters are already handled (BCS serialization with struct interfaces). Generating event types for those would be redundant and misleading.

**Rule:** A copy+drop struct is treated as an event if it is **not** referenced as a function parameter in the module. Copy+drop structs used as function params already get `export interface` + BCS encoding. The remaining copy+drop structs are events (or event-like data types emitted via `sui::event::emit()`).

This reuses the existing `collect_referenced_structs()` logic in codegen — structs referenced in function params are excluded from event generation.

### Generated Output

For each copy+drop struct, when `--events` is enabled:

```typescript
export type ItemPurchased = {
  buyer: string;
  price: string;
  itemId: string;
};
```

Key rules:
- Use `export type` (not `export interface`) — events are data shapes, not objects
- **ALL fields are `string`** — no Move type mapping. Event data from RPC/indexers is string-serialized.
- Field names: snake_case -> camelCase (same as function params)
- Struct name: PascalCase (preserved from Move source)

### Where to Place in Generated File

Event types go at the **end** of the generated `.ts` file, after function wrappers, separated by a comment:

```typescript
// ... imports, getters, interfaces, functions ...

// --- Event Types ---

export type ItemPurchased = {
  buyer: string;
  price: string;
  itemId: string;
};

export type ListingCreated = {
  seller: string;
  price: string;
  listingId: string;
};
```

## Implementation

### Files to Change

1. **`src/cli.rs`** — Add `--events` boolean flag
2. **`src/codegen.rs`** — Add `generate_event_types()` function. In `generate_module()`, when `include_events` is true, collect copy+drop structs NOT referenced as function params and render them as `export type` with all-string fields.
3. **`src/driver.rs`** — Pass `cli.events` through to `CodegenConfig`.

No IR changes needed — `StructInfo` already tracks `has_copy`/`has_drop` and field names. Event detection is a codegen-time filter, not an analyzer concern.

### CodegenConfig Change

```rust
pub struct CodegenConfig {
    pub package_id_env_var: String,
    pub project_name: String,
    pub include_events: bool,      // new
}
```

### Codegen Logic

```rust
fn generate_event_types(w: &mut CodeWriter, module: &ModuleInfo) {
    let referenced = collect_referenced_structs(module);
    let events: Vec<&StructInfo> = module.structs.iter()
        .filter(|s| s.is_pure_value() && !referenced.contains(&s.name))
        .collect();
    if events.is_empty() { return; }

    w.line("// --- Event Types ---");
    w.blank();
    for event in events {
        w.line(&format!("export type {} = {{", event.name));
        w.indent();
        for (field_name, _) in &event.fields {
            w.line(&format!("{}: string;", to_camel_case(field_name)));
        }
        w.dedent();
        w.line("};");
        w.blank();
    }
}
```

## Acceptance Criteria

- [ ] `--events` flag added to CLI
- [ ] Copy+drop structs NOT used as function params are rendered as event types
- [ ] Copy+drop structs used as function params remain as BCS interfaces (no duplication)
- [ ] Generated `export type` with all fields as `string`
- [ ] Field names converted to camelCase
- [ ] Event types only generated when `--events` flag is present
- [ ] Without `--events`, output is identical to current behavior
- [ ] Unit tests for event type generation
- [ ] Integration test with Move fixture containing events alongside value structs
- [ ] TS compilation test validates generated event types
