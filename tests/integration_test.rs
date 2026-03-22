use std::fs;
use std::process::Command;

use move2ts::analyzer::extract_modules;
use move2ts::codegen::{CodegenConfig, generate_errors_module, generate_module};
use move2ts::ir::TypeParamInfo;
use move2ts::parser::MoveParser;

fn parse_and_extract(source: &str) -> Vec<move2ts::ir::ModuleInfo> {
    let parser = MoveParser::new();
    let defs = parser.parse_source(source).expect("should parse");
    extract_modules(&defs)
}

#[test]
fn full_pipeline_marketplace() {
    let source = fs::read_to_string("tests/fixtures/marketplace.move").expect("fixture exists");
    let modules = parse_and_extract(&source);

    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.name, "marketplace");

    // Marketplace should be detected as singleton (only constructed in init)
    assert!(
        module.singletons.contains("Marketplace"),
        "Marketplace should be a singleton, got: {:?}",
        module.singletons
    );

    // Should have 3 callable functions (init is private, skipped)
    assert_eq!(
        module.functions.len(),
        3,
        "Expected 3 functions, got: {:?}",
        module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // list_item should be entry
    let list_item = module
        .functions
        .iter()
        .find(|f| f.name == "list_item")
        .expect("list_item exists");
    assert!(list_item.is_entry);
    // Should have 2 params after stripping TxContext: marketplace (singleton) + price
    assert_eq!(list_item.params.len(), 2);

    // marketplace param should be marked as singleton
    let marketplace_param = list_item
        .params
        .iter()
        .find(|p| p.name == "marketplace")
        .expect("marketplace param exists");
    assert!(marketplace_param.is_singleton);

    // get_price should have clock auto-injected
    let get_price = module
        .functions
        .iter()
        .find(|f| f.name == "get_price")
        .expect("get_price exists");
    assert!(get_price.has_clock_param);
    // Clock should be stripped from params
    assert_eq!(get_price.params.len(), 1); // only marketplace

    // Generate TS
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };
    let ts_output = generate_module(module, &config);

    // Verify key elements in generated TS
    assert!(ts_output.contains("import process from 'node:process';"));
    assert!(ts_output.contains("TransactionObjectInput"));
    assert!(ts_output.contains("TransactionResult"));
    assert!(ts_output.contains("function getPackageId()"));
    assert!(ts_output.contains("function getMarketplaceId()"));
    assert!(ts_output.contains("export function listItem("));
    assert!(ts_output.contains("export function getPrice("));
    assert!(ts_output.contains("export function cancelListing("));
    assert!(ts_output.contains("tx.object.clock()"));
    assert!(ts_output.contains("marketplace?: TransactionObjectInput"));
}

#[test]
fn full_pipeline_defi_generics() {
    let source = fs::read_to_string("tests/fixtures/defi.move").expect("fixture exists");
    let modules = parse_and_extract(&source);

    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.name, "defi");

    // swap should have 2 type params
    let swap = module
        .functions
        .iter()
        .find(|f| f.name == "swap")
        .expect("swap exists");
    assert_eq!(
        swap.type_params,
        vec![
            TypeParamInfo {
                name: "X".to_string(),
                has_key: false
            },
            TypeParamInfo {
                name: "Y".to_string(),
                has_key: false
            },
        ]
    );

    // withdraw should have 1 type param
    let withdraw = module
        .functions
        .iter()
        .find(|f| f.name == "withdraw")
        .expect("withdraw exists");
    assert_eq!(
        withdraw.type_params,
        vec![TypeParamInfo {
            name: "T".to_string(),
            has_key: false
        }]
    );

    // get_random_reward should have both clock and random
    let random_reward = module
        .functions
        .iter()
        .find(|f| f.name == "get_random_reward")
        .expect("get_random_reward exists");
    assert!(random_reward.has_clock_param);
    assert!(random_reward.has_random_param);

    // Generate TS and check
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };
    let ts_output = generate_module(module, &config);

    assert!(ts_output.contains("typeX: string;"));
    assert!(ts_output.contains("typeY: string;"));
    assert!(ts_output.contains("typeArguments:"));
    assert!(ts_output.contains("tx.object.clock()"));
    assert!(ts_output.contains("tx.object.random()"));
}

#[test]
fn full_pipeline_pure_structs() {
    let source = fs::read_to_string("tests/fixtures/pure_structs.move").expect("fixture exists");
    let modules = parse_and_extract(&source);

    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.name, "config");

    // Registry should be singleton (only constructed in init)
    assert!(
        module.singletons.contains("Registry"),
        "Registry should be a singleton, got: {:?}",
        module.singletons
    );

    // Config should NOT be singleton (copy+drop, no key — not an on-chain object)
    assert!(
        !module.singletons.contains("Config"),
        "Config should not be a singleton (pure value struct), got: {:?}",
        module.singletons
    );

    // Config struct should have copy+drop but no key
    let config_struct = module
        .structs
        .iter()
        .find(|s| s.name == "Config")
        .expect("Config struct exists");
    assert!(config_struct.has_copy);
    assert!(config_struct.has_drop);
    assert!(!config_struct.has_key);
    assert!(config_struct.is_pure_value());

    // Metadata struct should also be copy+drop
    let metadata_struct = module
        .structs
        .iter()
        .find(|s| s.name == "Metadata")
        .expect("Metadata struct exists");
    assert!(metadata_struct.has_copy);
    assert!(metadata_struct.has_drop);
    assert!(metadata_struct.is_pure_value());

    // Registry struct should have key (object)
    let registry_struct = module
        .structs
        .iter()
        .find(|s| s.name == "Registry")
        .expect("Registry struct exists");
    assert!(registry_struct.has_key);
    assert!(!registry_struct.is_pure_value());

    // update_config takes Config by value — should use BCS
    let update_config = module
        .functions
        .iter()
        .find(|f| f.name == "update_config")
        .expect("update_config exists");
    let config_param = update_config
        .params
        .iter()
        .find(|p| p.name == "new_config")
        .expect("new_config param exists");
    // Should be a Struct, not a Ref (passed by value)
    assert!(
        matches!(&config_param.move_type, move2ts::ir::MoveType::Struct { name, .. } if name == "Config"),
        "new_config should be MoveType::Struct, got: {:?}",
        config_param.move_type
    );

    // set_metadata should have clock auto-injected
    let set_metadata = module
        .functions
        .iter()
        .find(|f| f.name == "set_metadata")
        .expect("set_metadata exists");
    assert!(set_metadata.has_clock_param);

    // Generate TS and verify BCS usage
    let codegen_config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };
    let ts_output = generate_module(module, &codegen_config);

    // Should import bcs (pure value structs are used)
    assert!(
        ts_output.contains("import { bcs } from '@mysten/bcs'"),
        "should import bcs for pure value structs"
    );

    // update_config should use BCS serialization for Config param
    assert!(
        ts_output.contains("bcs.struct('Config'"),
        "should use bcs.struct for Config"
    );

    // Config BCS schema should include the correct field types
    assert!(
        ts_output.contains("bcs.u64()"),
        "Config schema should have u64 for max_items"
    );
    assert!(
        ts_output.contains("bcs.u16()"),
        "Config schema should have u16 for fee_bps"
    );
    assert!(
        ts_output.contains("bcs.bool()"),
        "Config schema should have bool for enabled"
    );

    // set_metadata should use BCS for Metadata param
    assert!(
        ts_output.contains("bcs.struct('Metadata'"),
        "should use bcs.struct for Metadata"
    );

    // Registry should still use tx.object (it has key)
    assert!(
        ts_output.contains("tx.object("),
        "Registry should use tx.object"
    );

    // Clock should be auto-injected in set_metadata
    assert!(ts_output.contains("tx.object.clock()"));

    // Registry should be singleton (optional param with TransactionObjectInput)
    assert!(ts_output.contains("registry?: TransactionObjectInput"));

    // Config param in update_config should use BCS (not singleton, pure value)
    // It should have the struct type, not TransactionObjectInput
    assert!(
        ts_output.contains("newConfig: Config;"),
        "Config param should use the Config interface type for BCS serialization"
    );
}

#[test]
fn full_pipeline_vecmap() {
    let source = fs::read_to_string("tests/fixtures/vecmap.move").expect("fixture exists");
    let modules = parse_and_extract(&source);

    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.name, "maps");

    // set_labels should have a VecMap<u64, bool> param
    let set_labels = module
        .functions
        .iter()
        .find(|f| f.name == "set_labels")
        .expect("set_labels exists");
    let labels_param = set_labels
        .params
        .iter()
        .find(|p| p.name == "labels")
        .expect("labels param exists");
    assert_eq!(
        labels_param.move_type,
        move2ts::ir::MoveType::VecMap(
            Box::new(move2ts::ir::MoveType::U64),
            Box::new(move2ts::ir::MoveType::Bool),
        )
    );

    // set_addresses should have VecMap<address, u64>
    let set_addresses = module
        .functions
        .iter()
        .find(|f| f.name == "set_addresses")
        .expect("set_addresses exists");
    let targets_param = set_addresses
        .params
        .iter()
        .find(|p| p.name == "targets")
        .expect("targets param exists");
    assert_eq!(
        targets_param.move_type,
        move2ts::ir::MoveType::VecMap(
            Box::new(move2ts::ir::MoveType::Address),
            Box::new(move2ts::ir::MoveType::U64),
        )
    );

    // Settings struct should have a VecMap field
    let settings = module
        .structs
        .iter()
        .find(|s| s.name == "Settings")
        .expect("Settings struct exists");
    assert!(settings.is_pure_value());
    let labels_field = settings
        .fields
        .iter()
        .find(|(name, _)| name == "labels")
        .expect("labels field exists");
    assert_eq!(
        labels_field.1,
        move2ts::ir::MoveType::VecMap(
            Box::new(move2ts::ir::MoveType::U64),
            Box::new(move2ts::ir::MoveType::Bool),
        )
    );

    // Generate TypeScript and verify output
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };
    let ts_output = generate_module(module, &config);

    // Should import bcs from @mysten/sui/bcs (includes both bcs and Address)
    assert!(
        ts_output.contains("import { bcs } from '@mysten/sui/bcs';"),
        "should import bcs from @mysten/sui/bcs (module has VecMap<address, u64>)"
    );

    // set_labels param should be Map<bigint, boolean>
    assert!(
        ts_output.contains("labels: Map<bigint, boolean>"),
        "VecMap<u64, bool> should map to Map<bigint, boolean>"
    );

    // set_addresses param should be Map<string, bigint>
    assert!(
        ts_output.contains("targets: Map<string, bigint>"),
        "VecMap<address, u64> should map to Map<string, bigint>"
    );

    // Settings interface should have labels field as Map
    assert!(
        ts_output.contains("labels: Map<bigint, boolean>;"),
        "Settings.labels should be Map<bigint, boolean>"
    );

    // BCS encoding should use bcs.map
    assert!(
        ts_output.contains("bcs.map(bcs.u64(), bcs.bool())"),
        "should use bcs.map for VecMap encoding"
    );

    // bcs.Address should be used in bcs.map for address keys
    assert!(
        ts_output.contains("bcs.map(bcs.Address, bcs.u64())"),
        "should use bcs.Address in bcs.map for address keys"
    );
}

#[test]
fn full_pipeline_events() {
    let source = fs::read_to_string("tests/fixtures/events.move").expect("fixture exists");
    let modules = parse_and_extract(&source);

    assert_eq!(modules.len(), 1);
    let module = &modules[0];
    assert_eq!(module.name, "marketplace_events");

    // Verify emitted events were detected from event::emit() calls
    assert!(
        module.emitted_events.contains("ItemPurchased"),
        "ItemPurchased should be detected as emitted, got: {:?}",
        module.emitted_events
    );
    assert!(module.emitted_events.contains("ListingCreated"));
    assert!(module.emitted_events.contains("FeeCollected"));

    // TradeInfo is both emitted AND used as function param
    assert!(module.emitted_events.contains("TradeInfo"));

    // PriceRange is NOT emitted (only used as function param)
    assert!(!module.emitted_events.contains("PriceRange"));

    // Generate with events enabled
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: true,
    };
    let ts_output = generate_module(module, &config);

    // Pure event types (not used as params) — no suffix
    assert!(ts_output.contains("export type ItemPurchased = {"));
    assert!(ts_output.contains("export type ListingCreated = {"));
    assert!(ts_output.contains("export type FeeCollected = {"));

    // All event fields should be readonly string
    assert!(ts_output.contains("readonly buyer: string;"));
    assert!(ts_output.contains("readonly price: string;"));

    // TradeInfo is both emitted AND a param — gets Event suffix
    assert!(
        ts_output.contains("export type TradeInfoEvent = {"),
        "TradeInfo should get Event suffix since it's also a function param"
    );
    // TradeInfo should also have BCS interface (for the param usage)
    assert!(ts_output.contains("export interface TradeInfo {"));

    // PriceRange is NOT emitted — no event type
    assert!(!ts_output.contains("export type PriceRange"));
    // PriceRange should still have BCS interface (used as param)
    assert!(ts_output.contains("export interface PriceRange {"));

    // Marketplace (key struct, not emitted) — no event type
    assert!(!ts_output.contains("export type Marketplace"));

    // Without --events, no event types
    let config_no_events = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };
    let ts_no_events = generate_module(module, &config_no_events);
    assert!(!ts_no_events.contains("Event Types"));
    assert!(!ts_no_events.contains("export type ItemPurchased"));
}

#[test]
fn errors_module_generates_valid_content() {
    let output = generate_errors_module();
    assert!(output.contains("export class InvalidConfigError extends Error"));
    assert!(output.contains("override readonly name = 'InvalidConfigError' as const;"));
    assert!(!output.contains("validateSuiAddress"));
}

/// Validates that generated TypeScript compiles with tsc --strict
/// against the real @mysten/sui type definitions.
///
/// Uses tests/ts-check/ which has typescript and @mysten/sui installed.
/// Run `pnpm install` in tests/ts-check/ before running this test.
#[test]
fn generated_ts_compiles_with_tsc() {
    let ts_check_dir = std::path::Path::new("tests/ts-check");

    // Check if ts-check environment is set up
    if !ts_check_dir.join("node_modules").exists() {
        eprintln!(
            "Note: tests/ts-check/node_modules not found. Run `cd tests/ts-check && pnpm install` first. Skipping."
        );
        return;
    }

    let generated_dir = ts_check_dir.join("generated");
    fs::create_dir_all(&generated_dir).expect("create generated dir");

    // Generate TS from both test fixtures
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: false,
    };

    // Marketplace module
    let marketplace_source =
        fs::read_to_string("tests/fixtures/marketplace.move").expect("fixture exists");
    let marketplace_modules = parse_and_extract(&marketplace_source);
    fs::write(
        generated_dir.join("marketplace.ts"),
        generate_module(&marketplace_modules[0], &config),
    )
    .expect("write marketplace.ts");

    // DeFi module
    let defi_source = fs::read_to_string("tests/fixtures/defi.move").expect("fixture exists");
    let defi_modules = parse_and_extract(&defi_source);
    fs::write(
        generated_dir.join("defi.ts"),
        generate_module(&defi_modules[0], &config),
    )
    .expect("write defi.ts");

    // Pure structs module (copy+drop structs with BCS serialization)
    let pure_source =
        fs::read_to_string("tests/fixtures/pure_structs.move").expect("fixture exists");
    let pure_modules = parse_and_extract(&pure_source);
    fs::write(
        generated_dir.join("config.ts"),
        generate_module(&pure_modules[0], &config),
    )
    .expect("write config.ts");

    // Events module (with --events enabled)
    let events_config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
        include_events: true,
    };
    let events_source = fs::read_to_string("tests/fixtures/events.move").expect("fixture exists");
    let events_modules = parse_and_extract(&events_source);
    fs::write(
        generated_dir.join("marketplace_events.ts"),
        generate_module(&events_modules[0], &events_config),
    )
    .expect("write marketplace_events.ts");

    // VecMap module
    let vecmap_source = fs::read_to_string("tests/fixtures/vecmap.move").expect("fixture exists");
    let vecmap_modules = parse_and_extract(&vecmap_source);
    fs::write(
        generated_dir.join("maps.ts"),
        generate_module(&vecmap_modules[0], &config),
    )
    .expect("write maps.ts");

    // Shared errors module
    fs::write(
        generated_dir.join("move2ts-errors.ts"),
        generate_errors_module(),
    )
    .expect("write move2ts-errors.ts");

    // Run tsc --strict against the real @mysten/sui types
    let tsc_result = Command::new("pnpm")
        .args(["exec", "tsc", "--noEmit"])
        .current_dir(ts_check_dir)
        .output()
        .expect("failed to run tsc");

    // Clean up generated files regardless of result
    let _ = fs::remove_dir_all(&generated_dir);

    if !tsc_result.status.success() {
        let stdout = String::from_utf8_lossy(&tsc_result.stdout);
        let stderr = String::from_utf8_lossy(&tsc_result.stderr);
        panic!("TypeScript compilation failed with tsc --strict:\n{stdout}\n{stderr}");
    }
}
