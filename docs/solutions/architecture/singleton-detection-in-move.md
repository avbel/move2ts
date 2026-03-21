---
title: Detecting singleton objects in Sui Move modules
category: architecture
date: 2026-03-21
tags: [singleton, init, move, sui, ast-analysis]
module: analyzer
symptom: Need to identify which Move structs are singletons to make their params optional with env var fallback
root_cause: No built-in metadata in Move for singletons — must be inferred from code patterns
---

## Problem

In Sui Move, some structs are singletons — created once in `init()` and never again. When generating TypeScript wrappers, these should be optional parameters backed by environment variables.

### Failed approaches

1. **Detect `transfer::share_object()` in init()** — too narrow. Singletons can be transferred (not shared), frozen, or wrapped in other objects.
2. **All copy+drop structs are singletons** — wrong. Copy+drop structs are pure values, not on-chain objects.
3. **All structs constructed in init() are singletons** — wrong. Copy+drop structs like `Config` can be constructed inside a `Registry` literal in init() but they're values, not on-chain objects.

## Solution

A struct is a singleton if:
1. It is **constructed in `init()`** AND
2. **No other function** in the module constructs it AND
3. It has the **`key` ability** (i.e., it's an on-chain object, not a pure value)

The `key` check is critical — without it, copy+drop structs nested inside object constructors (like `Config` inside `Registry { config: Config { ... } }`) would be falsely detected as singletons.

### Algorithm (two-pass for borrow checker safety)

```rust
// Pass 1: scan ALL functions for struct constructors (Pack expressions)
let constructor_map = build_constructor_map(module_def); // struct_name -> {function_names}

// Pass 2: filter to singletons
let singletons = constructor_map.iter()
    .filter(|(name, fns)| fns.len() == 1 && fns.contains("init"))
    .filter(|(name, _)| structs.iter().any(|s| s.name == *name && s.has_key))
    .map(|(name, _)| name.clone())
    .collect();
```

### CLI escape hatch

`--singletons Struct1,Struct2` for when the heuristic fails (e.g., helper functions construct the struct).

## Prevention

- Always check `has_key` when determining if a struct can be an on-chain singleton
- Test with modules that have copy+drop structs nested inside object constructors
- Document known limitations: helper functions calling constructors are not detected
