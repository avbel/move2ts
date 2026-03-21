# move2ts — Move-to-TypeScript Code Generator

**Date:** 2026-03-21
**Status:** Brainstorm

## What We're Building

A Rust CLI tool that parses Sui Move source files (`.move`) and generates type-safe TypeScript functions for calling `entry` and `public` methods via the `@mysten/sui` TypeScript SDK.

The generated code:
- Is portable across Node.js, Deno, and Bun
- Produces one `.ts` file per Move module
- Generates TS interfaces for Move structs used in function signatures
- Detects singleton objects (created and shared via `transfer::share_object` in `init()`) and makes them optional parameters backed by environment variables
- Uses a dedicated error type for missing configuration

## Why This Approach

### Parser: move-compiler crate (path dependency)

Use the `move-compiler` parser from `~/Projects/sui` as a Rust path dependency. This gives us the real, battle-tested Move AST without reimplementing any parsing logic.

**Trade-offs accepted:**
- Heavier compile times and dependency on local Sui repo clone — acceptable for a dev tool
- Tied to a specific Sui compiler version — manageable since the user controls the repo

**Rejected alternatives:**
- Custom parser (tree-sitter / hand-rolled) — duplicated effort, risk of syntax edge-case bugs
- move-analyzer LSP layer — too heavy for batch CLI use, awkward API fit

## Key Decisions

### Input/Output
- **Input:** `.move` source files or a Move package directory (with `Move.toml`) passed as CLI argument
- **Output:** One `.ts` file per Move module, written to an output directory

### Type Mapping (Move → TypeScript)
| Move Type | TypeScript Type |
|-----------|----------------|
| `u8`, `u16`, `u32` | `number` |
| `u64`, `u128`, `u256` | `bigint` |
| `bool` | `boolean` |
| `address` | `string` |
| `vector<u8>` | `Uint8Array` |
| `vector<T>` | `T[]` |
| `0x1::string::String` | `string` |
| `0x2::object::ID` | `string` |
| `Option<T>` | `T \| undefined` |
| `&T` / `&mut T` (objects) in `entry` fns | `string` (object ID) |
| `&T` / `&mut T` (objects) in `public` fns | `string \| TransactionResult` (object ID or prior tx result) |
| `Coin<T>` / `Balance<T>` in `entry` fns | `string` (object ID) |
| `Coin<T>` / `Balance<T>` in `public` fns | `string \| TransactionResult` (allows composing with splitCoins etc.) |
| Custom structs | Generated TS interface |

### Object Argument Types (entry vs public)
- **`entry` functions:** Object parameters accept `string` (object ID) only — entry functions cannot consume results from prior transaction commands
- **`public` functions:** Object parameters accept `string | TransactionResult` — allows composing with prior tx commands (e.g., passing result of `splitCoins` as a `Coin<T>` argument)

### Singleton Detection
- A struct is a singleton if it is constructed inside `init()` AND no other function in the module constructs it
- This means only `init()` can create it — regardless of whether it's shared, transferred, or frozen
- In generated functions, singleton parameters become optional (`objectId?: string`)
- If not provided, read from env variable using pattern `MODULE_STRUCT_ID` (e.g., `MY_MODULE_REGISTRY_ID`)
- If env var is also undefined, throw a dedicated error type (e.g., `BadArgumentError`)

### Package ID Handling
- Generated file starts with a const reading from env variable
- If env var not set, throw `BadArgumentError` at import/init time
- Env var pattern: `MODULE_PACKAGE_ID` (e.g., `MY_MODULE_PACKAGE_ID`)

### CLI Interface
```
move2ts <input> [options]

Arguments:
  <input>                      Move source file (.move) or package directory (with Move.toml)

Options:
  -o, --output <dir>           Output directory (default: ./generated)
  --methods <method1,method2>  Generate only these methods
  --skip-methods <m1,m2>       Skip these methods
```

### Generated Code Style
- ESM only (`export` / `import`)
- Uses `@mysten/sui` SDK (`Transaction` class, `moveCall`)
- No runtime dependencies beyond `@mysten/sui`
- Portable: works in Node.js, Deno (with `--allow-env` and Node compat), and Bun
- Generated files include `import process from 'node:process'` for explicit process access
- **Every generated function takes exactly 2 arguments:** `tx: Transaction` and an args object containing all parameters (including type args and optional singletons)
- Move function names are converted from `snake_case` to `camelCase` for generated TS functions (e.g., `list_item` → `listItem`)
- No AbortSignal — wrappers are synchronous (they append to a transaction, no network calls)

### Error Handling
- Generate a shared `move2ts-errors.ts` module with `BadArgumentError` class; each generated module imports from it
- Thrown at function call time (not import time) for singleton IDs
- Thrown at module load time for package ID (required for all calls)

### CLI Filtering
- `--methods` and `--skip-methods` use Move source names (`snake_case`, e.g., `--methods list_item,cancel_listing`)

### Generic Type Parameters
- Each Move type param generates a separate `string` parameter named after the type variable: `swap<X, Y>(...)` → `typeX: string, typeY: string`
- Passed to `moveCall` via `typeArguments: [typeX, typeY]`

### Generated Function Shape (Example)

Given Move:
```move
module my_package::marketplace {
    public struct Marketplace has key {
        id: UID,
        fee: u64,
    }

    public struct Listing has key, store {
        id: UID,
        price: u64,
        seller: address,
    }

    fun init(ctx: &mut TxContext) {
        let marketplace = Marketplace { id: object::new(ctx), fee: 100 };
        transfer::share_object(marketplace);
    }

    public entry fun list_item(
        marketplace: &mut Marketplace,
        price: u64,
        ctx: &mut TxContext,
    ) { ... }
}
```

Generated TypeScript:
```typescript
import process from 'node:process';
import { Transaction } from '@mysten/sui/transactions';
import { BadArgumentError } from './move2ts-errors';

const marketplacePackageId = process.env.MARKETPLACE_PACKAGE_ID;
if (!marketplacePackageId) {
  throw new BadArgumentError('MARKETPLACE_PACKAGE_ID environment variable is not set');
}

// Singleton: Marketplace is shared in init()
const marketplaceMarketplaceId = process.env.MARKETPLACE_MARKETPLACE_ID;

export interface Listing {
  id: string;
  price: bigint;
  seller: string;
}

export function listItem(
  tx: Transaction,
  args: {
    price: bigint;
    marketplaceId?: string;
  },
) {
  const resolvedMarketplaceId = args.marketplaceId ?? marketplaceMarketplaceId;
  if (!resolvedMarketplaceId) {
    throw new BadArgumentError(
      'marketplaceId must be provided or MARKETPLACE_MARKETPLACE_ID env var must be set'
    );
  }

  tx.moveCall({
    target: `${marketplacePackageId}::marketplace::list_item`,
    arguments: [
      tx.object(resolvedMarketplaceId),
      tx.pure.u64(args.price),
    ],
  });
}
```

## Resolved Questions

1. **TxContext and Clock parameters** — Auto-stripped from generated signatures. They are implicit in Sui transactions.
2. **Generic type parameters** — `public fun withdraw<T>(...)` generates a `typeArg: string` parameter where the caller passes the full Move type (e.g., `'0x2::sui::SUI'`).
3. **Input scope** — Supports both modes: pass a Move package directory (reads Move.toml, finds all `.move` files in sources/) or pass individual `.move` files directly.
