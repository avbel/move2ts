use std::fs;
use std::process::Command;

use move2ts::analyzer::extract_modules;
use move2ts::codegen::{CodegenConfig, generate_errors_module, generate_module};
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
    assert_eq!(swap.type_params, vec!["X", "Y"]);

    // withdraw should have 1 type param
    let withdraw = module
        .functions
        .iter()
        .find(|f| f.name == "withdraw")
        .expect("withdraw exists");
    assert_eq!(withdraw.type_params, vec!["T"]);

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
    };
    let ts_output = generate_module(module, &config);

    assert!(ts_output.contains("typeX: string;"));
    assert!(ts_output.contains("typeY: string;"));
    assert!(ts_output.contains("typeArguments:"));
    assert!(ts_output.contains("tx.object.clock()"));
    assert!(ts_output.contains("tx.object.random()"));
}

#[test]
fn errors_module_generates_valid_content() {
    let output = generate_errors_module();
    assert!(output.contains("export class Move2TsConfigError extends Error"));
    assert!(output.contains("override readonly name = 'Move2TsConfigError' as const;"));
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
        eprintln!("Note: tests/ts-check/node_modules not found. Run `cd tests/ts-check && pnpm install` first. Skipping.");
        return;
    }

    let generated_dir = ts_check_dir.join("generated");
    fs::create_dir_all(&generated_dir).expect("create generated dir");

    // Generate TS from both test fixtures
    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
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
