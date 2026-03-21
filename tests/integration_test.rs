use std::fs;
use std::process::Command;

use move2ts::analyzer::extract_modules;
use move2ts::codegen::{generate_errors_module, generate_module, CodegenConfig};
use move2ts::parser::MoveParser;

fn parse_and_extract(source: &str) -> Vec<move2ts::analyzer::ModuleInfo> {
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
    assert!(output.contains("export function validateSuiAddress"));
    assert!(output.contains("/^0x[0-9a-fA-F]{1,64}$/"));
}

#[test]
fn generated_ts_is_valid_typescript() {
    let source = fs::read_to_string("tests/fixtures/marketplace.move").expect("fixture exists");
    let modules = parse_and_extract(&source);
    let module = &modules[0];

    let config = CodegenConfig {
        package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
        project_name: "my_project".to_string(),
    };

    // Write generated files to temp dir
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let ts_path = temp_dir.path().join("marketplace.ts");
    let errors_path = temp_dir.path().join("move2ts-errors.ts");
    let tsconfig_path = temp_dir.path().join("tsconfig.json");

    fs::write(&ts_path, generate_module(module, &config)).expect("write ts");
    fs::write(&errors_path, generate_errors_module()).expect("write errors");
    fs::write(
        &tsconfig_path,
        r#"{
  "compilerOptions": {
    "strict": true,
    "noEmit": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "target": "ES2022",
    "skipLibCheck": true,
    "types": [],
    "paths": {
      "node:process": ["./node_modules/node_process/index.d.ts"]
    }
  },
  "include": ["*.ts"]
}"#,
    )
    .expect("write tsconfig");

    // Write stub for node:process
    let node_modules = temp_dir.path().join("node_modules");
    let process_dir = node_modules.join("node_process");
    fs::create_dir_all(&process_dir).expect("create process dir");
    fs::write(
        process_dir.join("index.d.ts"),
        r#"
declare const process: {
  env: Record<string, string | undefined>;
};
export default process;
"#,
    )
    .expect("write process stubs");

    // Write stub type declarations for @mysten/sui
    let sui_dir = node_modules.join("@mysten/sui");
    fs::create_dir_all(sui_dir.join("transactions")).expect("create dirs");
    fs::write(
        sui_dir.join("transactions/index.d.ts"),
        r#"
export type TransactionObjectInput = string | { $kind: string };
export type TransactionResult = { $kind: "Result"; Result: number } & { $kind: "NestedResult"; NestedResult: [number, number] }[];

interface ObjectHelper {
  (id: TransactionObjectInput): any;
  clock(): any;
  random(): any;
}

export declare class Transaction {
  moveCall(args: {
    target: string;
    typeArguments?: string[];
    arguments?: any[];
  }): TransactionResult;
  object: ObjectHelper;
  pure: {
    u8(v: number): any;
    u16(v: number): any;
    u32(v: number): any;
    u64(v: bigint): any;
    u128(v: bigint): any;
    u256(v: bigint): any;
    bool(v: boolean): any;
    address(v: string): any;
    string(v: string): any;
    id(v: string): any;
    vector(type: string, values: any): any;
    option(type: string, value: any): any;
    (type: string, value: any): any;
  };
}
"#,
    )
    .expect("write sui stubs");

    fs::write(
        sui_dir.join("package.json"),
        r#"{"name": "@mysten/sui", "exports": {"./transactions": "./transactions/index.d.ts"}}"#,
    )
    .expect("write package.json");

    // Install typescript locally and run tsc
    let install_result = Command::new("pnpm")
        .args(["add", "-D", "typescript"])
        .current_dir(temp_dir.path())
        .output();

    if install_result.is_err() {
        eprintln!("Note: pnpm not available, skipping TypeScript compilation check");
        return;
    }

    let tsc_result = Command::new("pnpm")
        .args(["exec", "tsc", "--project", tsconfig_path.to_str().unwrap()])
        .current_dir(temp_dir.path())
        .output();

    match tsc_result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stderr.contains("not found") && !stderr.contains("ENOENT") {
                    panic!(
                        "TypeScript compilation failed:\nstdout: {stdout}\nstderr: {stderr}\n\nGenerated TS:\n{}",
                        fs::read_to_string(&ts_path).unwrap_or_default()
                    );
                }
            }
        }
        Err(_) => {
            eprintln!("Note: tsc not available, skipping TypeScript compilation check");
        }
    }
}
