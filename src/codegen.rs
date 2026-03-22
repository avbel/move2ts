use std::collections::HashSet;

use convert_case::{Case, Casing};

use crate::ir::{FunctionInfo, ModuleInfo, MoveType, StructInfo};

/// Configuration for code generation.
pub struct CodegenConfig {
    pub package_id_env_var: String,
    pub project_name: String,
    pub include_events: bool,
}

/// Simple code writer with indentation support.
struct CodeWriter {
    buffer: String,
    indent: usize,
}

impl CodeWriter {
    fn new() -> Self {
        Self {
            buffer: String::with_capacity(16 * 1024),
            indent: 0,
        }
    }

    fn line(&mut self, content: &str) {
        if content.is_empty() {
            self.buffer.push('\n');
        } else {
            for _ in 0..self.indent {
                self.buffer.push_str("  ");
            }
            self.buffer.push_str(content);
            self.buffer.push('\n');
        }
    }

    fn indent(&mut self) {
        self.indent += 1;
    }

    fn dedent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    fn blank(&mut self) {
        self.buffer.push('\n');
    }

    fn into_string(self) -> String {
        self.buffer
    }
}

/// Maps a MoveType to its TypeScript type string.
pub fn to_ts_type(ty: &MoveType) -> String {
    match ty {
        MoveType::U8 | MoveType::U16 | MoveType::U32 => "number".to_string(),
        MoveType::U64 | MoveType::U128 | MoveType::U256 => "bigint".to_string(),
        MoveType::Bool => "boolean".to_string(),
        MoveType::Address => "string".to_string(),
        MoveType::SuiString => "string".to_string(),
        MoveType::ObjectId => "string".to_string(),
        MoveType::Vector(inner) if **inner == MoveType::U8 => "Uint8Array".to_string(),
        MoveType::Vector(inner) => {
            let inner_ts = to_ts_type(inner);
            // Wrap union types in parens when used as array element
            if inner_ts.contains(" | ") {
                format!("({inner_ts})[]")
            } else {
                format!("{inner_ts}[]")
            }
        }
        MoveType::Option(inner) => format!("{} | null", to_ts_type(inner)),
        MoveType::VecMap(key, value) => format!("Map<{}, {}>", to_ts_type(key), to_ts_type(value)),
        MoveType::Ref { .. } => "TransactionObjectInput".to_string(),
        MoveType::TypeParam { has_key, .. } => {
            if *has_key {
                "TransactionObjectInput".to_string()
            } else {
                "string".to_string()
            }
        }
        MoveType::Struct { name, .. } => {
            // Default: return the struct name. Use to_ts_type_for_param() for
            // context-aware resolution that distinguishes objects from pure values.
            name.clone()
        }
        MoveType::Unit => "void".to_string(),
    }
}

/// Maps a MoveType to its TypeScript type for function parameters.
/// Unlike `to_ts_type`, this uses module context: external structs (not in the module's
/// own pure value structs) default to TransactionObjectInput since they're objects.
/// Only the module's own copy+drop structs get their interface name.
fn to_ts_type_for_param(ty: &MoveType, own_pure_structs: &HashSet<String>) -> String {
    match ty {
        MoveType::Struct { name, .. } => {
            if own_pure_structs.contains(name.as_str()) {
                name.clone()
            } else {
                "TransactionObjectInput".to_string()
            }
        }
        _ => to_ts_type(ty),
    }
}

/// Maps a MoveType to its tx.pure.* or tx.object() encoding call.
pub fn to_tx_encoding(ty: &MoveType, expr: &str) -> String {
    match ty {
        MoveType::U8 => format!("tx.pure.u8({expr})"),
        MoveType::U16 => format!("tx.pure.u16({expr})"),
        MoveType::U32 => format!("tx.pure.u32({expr})"),
        MoveType::U64 => format!("tx.pure.u64({expr})"),
        MoveType::U128 => format!("tx.pure.u128({expr})"),
        MoveType::U256 => format!("tx.pure.u256({expr})"),
        MoveType::Bool => format!("tx.pure.bool({expr})"),
        MoveType::Address => format!("tx.pure.address({expr})"),
        MoveType::SuiString => format!("tx.pure.string({expr})"),
        MoveType::ObjectId => format!("tx.pure.id({expr})"),
        MoveType::Vector(inner) if **inner == MoveType::U8 => {
            format!("tx.pure('vector<u8>', {expr})")
        }
        MoveType::Vector(inner) if type_contains_vecmap(inner) => {
            format!(
                "tx.pure(bcs.vector({}).serialize({expr}))",
                to_bcs_schema(inner, &[])
            )
        }
        MoveType::Vector(inner) => {
            let inner_bcs = to_bcs_type_string(inner);
            format!("tx.pure.vector('{inner_bcs}', {expr})")
        }
        MoveType::Option(inner) if type_contains_vecmap(inner) => {
            format!(
                "tx.pure(bcs.option({}).serialize({expr}))",
                to_bcs_schema(inner, &[])
            )
        }
        MoveType::Option(inner) => {
            let inner_bcs = to_bcs_type_string(inner);
            format!("tx.pure.option('{inner_bcs}', {expr})")
        }
        MoveType::VecMap(key, value) => {
            format!(
                "tx.pure(bcs.map({}, {}).serialize({expr}))",
                to_bcs_schema(key, &[]),
                to_bcs_schema(value, &[])
            )
        }
        MoveType::Ref { .. } => format!("tx.object({expr})"),
        MoveType::TypeParam { has_key, .. } => {
            if *has_key {
                format!("tx.object({expr})")
            } else {
                format!("tx.pure({expr})")
            }
        }
        MoveType::Struct { .. } => format!("tx.object({expr})"), // assume object
        MoveType::Unit => String::new(),
    }
}

/// Maps MoveType to BCS schema builder call for @mysten/bcs.
/// Accepts module structs to recursively expand nested pure value structs.
fn to_bcs_schema(ty: &MoveType, structs: &[StructInfo]) -> String {
    match ty {
        MoveType::U8 => "bcs.u8()".to_string(),
        MoveType::U16 => "bcs.u16()".to_string(),
        MoveType::U32 => "bcs.u32()".to_string(),
        MoveType::U64 => "bcs.u64()".to_string(),
        MoveType::U128 => "bcs.u128()".to_string(),
        MoveType::U256 => "bcs.u256()".to_string(),
        MoveType::Bool => "bcs.bool()".to_string(),
        MoveType::Address => "bcs.Address".to_string(),
        MoveType::SuiString => "bcs.string()".to_string(),
        MoveType::ObjectId => "bcs.Address".to_string(),
        MoveType::Vector(inner) => format!("bcs.vector({})", to_bcs_schema(inner, structs)),
        MoveType::Option(inner) => format!("bcs.option({})", to_bcs_schema(inner, structs)),
        MoveType::VecMap(key, value) => {
            format!(
                "bcs.map({}, {})",
                to_bcs_schema(key, structs),
                to_bcs_schema(value, structs)
            )
        }
        MoveType::Struct { name, .. } => {
            if let Some(si) = structs
                .iter()
                .find(|s| s.name == *name && s.is_pure_value())
            {
                to_bcs_struct_schema(si, structs)
            } else {
                "bcs.bytes(32)".to_string() // fallback for unknown/external structs
            }
        }
        _ => "bcs.bytes(32)".to_string(), // fallback for unknown
    }
}

/// Generates a BCS struct schema expression for a pure value struct.
/// Returns something like: `bcs.struct('Name', { f1: bcs.u64(), f2: bcs.bool() })`
fn to_bcs_struct_schema(struct_info: &StructInfo, structs: &[StructInfo]) -> String {
    let field_schemas: Vec<String> = struct_info
        .fields
        .iter()
        .map(|(name, ty)| format!("{}: {}", to_camel_case(name), to_bcs_schema(ty, structs)))
        .collect();
    let fields_str = field_schemas.join(", ");
    format!("bcs.struct('{}', {{ {} }})", struct_info.name, fields_str)
}

/// Generates a BCS struct serialization call for a pure value struct.
/// Returns something like: `tx.pure(bcs.struct('Name', { f1: bcs.u64(), f2: bcs.bool() }).serialize(expr))`
fn to_bcs_struct_encoding(struct_info: &StructInfo, expr: &str, structs: &[StructInfo]) -> String {
    format!(
        "tx.pure({}.serialize({}))",
        to_bcs_struct_schema(struct_info, structs),
        expr
    )
}

/// Maps a MoveType to its tx.pure.* or tx.object() encoding call,
/// with context about the module's structs to distinguish pure value structs from objects.
fn to_tx_encoding_with_context(ty: &MoveType, expr: &str, structs: &[StructInfo]) -> String {
    match ty {
        MoveType::Struct { name, .. } => {
            // Look up the struct to determine if it's a pure value type
            if let Some(si) = structs.iter().find(|s| s.name == *name)
                && si.is_pure_value()
            {
                return to_bcs_struct_encoding(si, expr, structs);
            }
            // Default: treat as object
            format!("tx.object({expr})")
        }
        MoveType::VecMap(key, value) => {
            format!(
                "tx.pure(bcs.map({}, {}).serialize({expr}))",
                to_bcs_schema(key, structs),
                to_bcs_schema(value, structs)
            )
        }
        // For Vector/Option containing VecMap, use bcs schema path with struct context
        MoveType::Vector(inner) if type_contains_vecmap(inner) => {
            format!(
                "tx.pure(bcs.vector({}).serialize({expr}))",
                to_bcs_schema(inner, structs)
            )
        }
        MoveType::Option(inner) if type_contains_vecmap(inner) => {
            format!(
                "tx.pure(bcs.option({}).serialize({expr}))",
                to_bcs_schema(inner, structs)
            )
        }
        _ => to_tx_encoding(ty, expr),
    }
}

/// Returns true if a MoveType contains Address or ObjectId anywhere in its tree,
/// including inside nested pure value structs from the same module.
fn type_contains_address(ty: &MoveType, structs: &[StructInfo]) -> bool {
    match ty {
        MoveType::Address | MoveType::ObjectId => true,
        MoveType::Vector(inner) | MoveType::Option(inner) => type_contains_address(inner, structs),
        MoveType::VecMap(key, value) => {
            type_contains_address(key, structs) || type_contains_address(value, structs)
        }
        MoveType::Struct { name, .. } => {
            if let Some(si) = structs
                .iter()
                .find(|s| s.name == *name && s.is_pure_value())
            {
                si.fields
                    .iter()
                    .any(|(_, ty)| type_contains_address(ty, structs))
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Returns true if a MoveType contains VecMap anywhere in its tree.
fn type_contains_vecmap(ty: &MoveType) -> bool {
    match ty {
        MoveType::VecMap(_, _) => true,
        MoveType::Vector(inner) | MoveType::Option(inner) => type_contains_vecmap(inner),
        _ => false,
    }
}

/// Returns true if any VecMap in the type tree has Address/ObjectId in its key or value types.
fn param_vecmap_contains_address(ty: &MoveType, structs: &[StructInfo]) -> bool {
    match ty {
        MoveType::VecMap(key, value) => {
            type_contains_address(key, structs) || type_contains_address(value, structs)
        }
        MoveType::Vector(inner) | MoveType::Option(inner) => {
            param_vecmap_contains_address(inner, structs)
        }
        _ => false,
    }
}

/// Produces the BCS type string for nested pure encoding (e.g., 'vector<u64>', 'option<address>').
fn to_bcs_type_string(ty: &MoveType) -> String {
    match ty {
        MoveType::U8 => "u8".to_string(),
        MoveType::U16 => "u16".to_string(),
        MoveType::U32 => "u32".to_string(),
        MoveType::U64 => "u64".to_string(),
        MoveType::U128 => "u128".to_string(),
        MoveType::U256 => "u256".to_string(),
        MoveType::Bool => "bool".to_string(),
        MoveType::Address => "address".to_string(),
        MoveType::SuiString => "string".to_string(),
        MoveType::ObjectId => "address".to_string(),
        MoveType::Vector(inner) => format!("vector<{}>", to_bcs_type_string(inner)),
        MoveType::Option(inner) => format!("option<{}>", to_bcs_type_string(inner)),
        MoveType::VecMap(_, _) => {
            panic!("VecMap has no BCS type string — use to_bcs_schema() instead")
        }
        _ => panic!("unsupported BCS type: {ty:?}"),
    }
}

/// Validates that a name is a safe identifier (alphanumeric + underscores only).
/// Prevents code injection via malicious module/function/struct names.
pub fn validate_identifier(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("identifier must not be empty");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!(
            "identifier '{name}' contains unsafe characters (only alphanumeric and _ allowed)"
        );
    }
    Ok(())
}

/// Converts a Move snake_case name to TypeScript camelCase.
pub fn to_camel_case(name: &str) -> String {
    name.to_case(Case::Camel)
}

/// Converts a name to UPPER_SNAKE_CASE for env var naming.
pub fn to_env_var_name(name: &str) -> String {
    name.to_case(Case::UpperSnake)
}

// ============================================================================
// Full TS File Generation
// ============================================================================

/// Generates a complete TypeScript module file for a given `ModuleInfo`.
pub fn generate_module(module: &ModuleInfo, config: &CodegenConfig) -> String {
    let mut w = CodeWriter::new();

    // --- Auto-generated header ---
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    w.line("// Auto-generated by move2ts — do not edit manually");
    w.line(&format!("// Generated at: {now}"));
    w.blank();

    // Collect pure value structs referenced by function params (for BCS + Address import detection)
    let used_pure_structs: Vec<&StructInfo> = module
        .structs
        .iter()
        .filter(|s| {
            s.is_pure_value()
                && module.functions.iter().any(|f| {
                    f.params.iter().any(|p| {
                        matches!(&p.move_type, MoveType::Struct { name, .. } if *name == s.name)
                    })
                })
        })
        .collect();

    // Check if any function param transitively contains VecMap
    let has_vecmap_param = module
        .functions
        .iter()
        .any(|f| f.params.iter().any(|p| type_contains_vecmap(&p.move_type)));

    let needs_bcs = !used_pure_structs.is_empty() || has_vecmap_param;
    let needs_address = used_pure_structs.iter().any(|s| {
        s.fields
            .iter()
            .any(|(_, ty)| type_contains_address(ty, &module.structs))
    }) || module.functions.iter().any(|f| {
        f.params
            .iter()
            .any(|p| param_vecmap_contains_address(&p.move_type, &module.structs))
    });

    // --- Imports ---
    w.line("import process from 'node:process';");
    w.line(
        "import type { TransactionObjectInput, TransactionResult } from '@mysten/sui/transactions';",
    );
    w.line("import { Transaction } from '@mysten/sui/transactions';");
    w.line("import { isValidSuiAddress } from '@mysten/sui/utils';");
    w.line("import { InvalidConfigError } from './move2ts-errors';");
    if needs_bcs && needs_address {
        // @mysten/sui/bcs re-exports @mysten/bcs plus Sui-specific types (Address, etc.)
        w.line("import { bcs } from '@mysten/sui/bcs';");
    } else if needs_bcs {
        w.line("import { bcs } from '@mysten/bcs';");
    }
    w.blank();

    // --- Event types (after imports) ---
    let referenced_structs = collect_referenced_structs(module);
    if config.include_events {
        generate_event_types(&mut w, module, &referenced_structs);
    }

    // --- Struct interfaces (only for structs referenced in function params) ---
    for struct_info in &module.structs {
        if referenced_structs.contains(&struct_info.name) {
            generate_struct_interface(&mut w, struct_info);
            w.blank();
        }
    }

    // --- Exported function wrappers ---
    for func in &module.functions {
        generate_function_wrapper(&mut w, func, &module.name, &module.structs);
        w.blank();
    }

    // --- Internal helpers (package ID + singleton getters) ---
    generate_package_id_getter(&mut w, &config.package_id_env_var);
    w.blank();

    for singleton_name in &module.singletons {
        generate_singleton_getter(&mut w, &config.project_name, singleton_name);
        w.blank();
    }

    w.into_string()
}

/// Generates `export type` declarations for event structs.
/// Only structs that are actually emitted via `event::emit()` are included.
/// All fields are typed as `string` (event data from RPC/indexers is string-serialized).
/// If a struct is both emitted AND used as a function param, the event type gets an `Event` suffix
/// (the param version already has an `export interface` with proper Move type mapping).
fn generate_event_types(w: &mut CodeWriter, module: &ModuleInfo, referenced: &HashSet<String>) {
    let events: Vec<&StructInfo> = module
        .structs
        .iter()
        .filter(|s| module.emitted_events.contains(&s.name))
        .collect();

    if events.is_empty() {
        return;
    }

    for event in events {
        // If also used as a function param, add "Event" suffix to avoid name collision
        let type_name = if referenced.contains(&event.name) {
            format!("{}Event", event.name)
        } else {
            event.name.clone()
        };
        w.line(&format!("export type {type_name} = {{"));
        w.indent();
        for (field_name, _) in &event.fields {
            w.line(&format!("readonly {field_name}: string;"));
        }
        w.dedent();
        w.line("};");
        w.blank();
    }
}

/// Generates the `move2ts-errors.ts` module content.
pub fn generate_errors_module() -> String {
    let mut w = CodeWriter::new();

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    w.line("// Auto-generated by move2ts — do not edit manually");
    w.line(&format!("// Generated at: {now}"));
    w.blank();

    w.line("export class InvalidConfigError extends Error {");
    w.indent();
    w.line("override readonly name = 'InvalidConfigError' as const;");
    w.line("constructor(message: string) {");
    w.indent();
    w.line("super(message);");
    w.dedent();
    w.line("}");
    w.dedent();
    w.line("}");
    w.blank();

    w.into_string()
}

/// Generates the package ID getter function.
fn generate_package_id_getter(w: &mut CodeWriter, env_var_name: &str) {
    w.line("function getPackageId(): string {");
    w.indent();
    w.line(&format!("const id = process.env.{env_var_name};"));
    w.line("if (!id) {");
    w.indent();
    w.line(&format!(
        "throw new InvalidConfigError('{env_var_name} environment variable is not set');"
    ));
    w.dedent();
    w.line("}");
    w.line("if (!isValidSuiAddress(id)) {");
    w.indent();
    w.line(&format!(
        "throw new InvalidConfigError(`{env_var_name} is not a valid Sui address: ${{id}}`);"
    ));
    w.dedent();
    w.line("}");
    w.line("return id;");
    w.dedent();
    w.line("}");
}

/// Generates a singleton lazy getter function.
fn generate_singleton_getter(w: &mut CodeWriter, project_name: &str, struct_name: &str) {
    let env_var = format!(
        "{}_{}",
        to_env_var_name(project_name),
        to_env_var_name(struct_name)
    );
    let getter_name = format!("get{}Id", struct_name);

    w.line(&format!("function {getter_name}(): string {{"));
    w.indent();
    w.line(&format!("const id = process.env.{env_var};"));
    w.line("if (!id) {");
    w.indent();
    w.line(&format!(
        "throw new InvalidConfigError('{env_var} environment variable is not set');"
    ));
    w.dedent();
    w.line("}");
    w.line("if (!isValidSuiAddress(id)) {");
    w.indent();
    w.line(&format!(
        "throw new InvalidConfigError(`{env_var} is not a valid Sui address: ${{id}}`);"
    ));
    w.dedent();
    w.line("}");
    w.line("return id;");
    w.dedent();
    w.line("}");
}

/// Generates a TypeScript interface for a Move struct.
fn generate_struct_interface(w: &mut CodeWriter, struct_info: &StructInfo) {
    w.line(&format!("export interface {} {{", struct_info.name));
    w.indent();
    for (field_name, field_type) in &struct_info.fields {
        let ts_type = if field_name == "id" && struct_info.has_key {
            "string".to_string()
        } else {
            to_ts_type(field_type)
        };
        w.line(&format!("{}: {};", to_camel_case(field_name), ts_type));
    }
    w.dedent();
    w.line("}");
}

/// Collects the set of struct names that are referenced in function parameters.
fn collect_referenced_structs(module: &ModuleInfo) -> HashSet<String> {
    let mut referenced = HashSet::new();
    let struct_names: HashSet<&str> = module.structs.iter().map(|s| s.name.as_str()).collect();

    for func in &module.functions {
        for param in &func.params {
            collect_struct_refs_from_type(&param.move_type, &struct_names, &mut referenced);
        }
    }

    referenced
}

/// Recursively collects struct names from a MoveType.
fn collect_struct_refs_from_type(
    ty: &MoveType,
    struct_names: &HashSet<&str>,
    referenced: &mut HashSet<String>,
) {
    match ty {
        MoveType::Ref { .. } => {
            // Ref types map to TransactionObjectInput (string object ID),
            // not to the struct interface. Don't collect them.
        }
        MoveType::Struct {
            name, type_args, ..
        } => {
            if struct_names.contains(name.as_str()) {
                referenced.insert(name.clone());
            }
            for ta in type_args {
                collect_struct_refs_from_type(ta, struct_names, referenced);
            }
        }
        MoveType::Vector(inner) => {
            collect_struct_refs_from_type(inner, struct_names, referenced);
        }
        MoveType::Option(inner) => {
            collect_struct_refs_from_type(inner, struct_names, referenced);
        }
        MoveType::VecMap(key, value) => {
            collect_struct_refs_from_type(key, struct_names, referenced);
            collect_struct_refs_from_type(value, struct_names, referenced);
        }
        _ => {}
    }
}

/// Generates a TypeScript function wrapper for a Move function.
fn generate_function_wrapper(
    w: &mut CodeWriter,
    func: &FunctionInfo,
    module_name: &str,
    structs: &[StructInfo],
) {
    let ts_name = to_camel_case(&func.name);
    let has_args = !func.params.is_empty() || !func.type_params.is_empty();

    // Collect the module's own pure value struct names for type resolution
    let own_pure_structs: HashSet<String> = structs
        .iter()
        .filter(|s| s.is_pure_value())
        .map(|s| s.name.clone())
        .collect();

    // Build args type entries
    let mut arg_entries: Vec<String> = Vec::new();

    // Type params
    for tp in &func.type_params {
        let ts_param_name = format!("type{}", tp.name.to_case(Case::Pascal));
        arg_entries.push(format!("{ts_param_name}: string;"));
    }

    // Regular params
    for param in &func.params {
        let ts_param_name = to_camel_case(&param.name);
        let ts_type = if param.is_singleton {
            "TransactionObjectInput".to_string()
        } else {
            to_ts_type_for_param(&param.move_type, &own_pure_structs)
        };
        let optional = if param.is_singleton { "?" } else { "" };
        arg_entries.push(format!("{ts_param_name}{optional}: {ts_type};"));
    }

    // Function signature
    if has_args {
        w.line(&format!("export function {ts_name}("));
        w.indent();
        w.line("tx: Transaction,");
        w.line("args: {");
        w.indent();
        for entry in &arg_entries {
            w.line(entry);
        }
        w.dedent();
        w.line("},");
        w.dedent();
        w.line("): TransactionResult {");
    } else {
        w.line(&format!(
            "export function {ts_name}(tx: Transaction): TransactionResult {{"
        ));
    }

    w.indent();

    // Build moveCall arguments
    let mut move_args: Vec<String> = Vec::new();

    for param in &func.params {
        let ts_param_name = to_camel_case(&param.name);
        let expr = if param.is_singleton {
            // Singletons are always on-chain objects referenced by ID — use tx.object()
            let struct_name = param.move_type.struct_name().unwrap_or(&param.name);
            let getter_name = format!("get{struct_name}Id");
            let full_expr = format!("args.{ts_param_name} ?? {getter_name}()");
            format!("tx.object({full_expr})")
        } else {
            let accessor = format!("args.{ts_param_name}");
            to_tx_encoding_with_context(&param.move_type, &accessor, structs)
        };
        move_args.push(expr);
    }

    // Auto-inject Clock
    if func.has_clock_param {
        move_args.push("tx.object.clock()".to_string());
    }

    // Auto-inject Random
    if func.has_random_param {
        move_args.push("tx.object.random()".to_string());
    }

    // Build typeArguments
    let type_args: Vec<String> = func
        .type_params
        .iter()
        .map(|tp| {
            let ts_param_name = format!("type{}", tp.name.to_case(Case::Pascal));
            format!("args.{ts_param_name}")
        })
        .collect();

    // Generate moveCall
    w.line("return tx.moveCall({");
    w.indent();
    w.line(&format!(
        "target: `${{getPackageId()}}::{module_name}::{}`,",
        func.name
    ));

    if !type_args.is_empty() {
        w.line(&format!("typeArguments: [{}],", type_args.join(", ")));
    }

    if !move_args.is_empty() {
        w.line("arguments: [");
        w.indent();
        for arg in &move_args {
            w.line(&format!("{arg},"));
        }
        w.dedent();
        w.line("],");
    }

    w.dedent();
    w.line("});");
    w.dedent();
    w.line("}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ParamInfo, TypeParamInfo};

    // ---- to_ts_type tests ----

    #[test]
    fn primitives_map_correctly() {
        assert_eq!(to_ts_type(&MoveType::U8), "number");
        assert_eq!(to_ts_type(&MoveType::U16), "number");
        assert_eq!(to_ts_type(&MoveType::U32), "number");
        assert_eq!(to_ts_type(&MoveType::U64), "bigint");
        assert_eq!(to_ts_type(&MoveType::U128), "bigint");
        assert_eq!(to_ts_type(&MoveType::U256), "bigint");
        assert_eq!(to_ts_type(&MoveType::Bool), "boolean");
        assert_eq!(to_ts_type(&MoveType::Address), "string");
    }

    #[test]
    fn sui_string_maps_to_string() {
        assert_eq!(to_ts_type(&MoveType::SuiString), "string");
    }

    #[test]
    fn object_id_maps_to_string() {
        assert_eq!(to_ts_type(&MoveType::ObjectId), "string");
    }

    #[test]
    fn vector_u8_maps_to_uint8array() {
        let ty = MoveType::Vector(Box::new(MoveType::U8));
        assert_eq!(to_ts_type(&ty), "Uint8Array");
    }

    #[test]
    fn vector_u64_maps_to_bigint_array() {
        let ty = MoveType::Vector(Box::new(MoveType::U64));
        assert_eq!(to_ts_type(&ty), "bigint[]");
    }

    #[test]
    fn vector_address_maps_to_string_array() {
        let ty = MoveType::Vector(Box::new(MoveType::Address));
        assert_eq!(to_ts_type(&ty), "string[]");
    }

    #[test]
    fn option_u64_maps_to_nullable_bigint() {
        let ty = MoveType::Option(Box::new(MoveType::U64));
        assert_eq!(to_ts_type(&ty), "bigint | null");
    }

    #[test]
    fn option_vector_u8_maps_to_nullable_uint8array() {
        let ty = MoveType::Option(Box::new(MoveType::Vector(Box::new(MoveType::U8))));
        assert_eq!(to_ts_type(&ty), "Uint8Array | null");
    }

    #[test]
    fn option_vector_u64_maps_to_nullable_bigint_array() {
        let ty = MoveType::Option(Box::new(MoveType::Vector(Box::new(MoveType::U64))));
        assert_eq!(to_ts_type(&ty), "bigint[] | null");
    }

    #[test]
    fn vector_option_address() {
        let ty = MoveType::Vector(Box::new(MoveType::Option(Box::new(MoveType::Address))));
        assert_eq!(to_ts_type(&ty), "(string | null)[]");
    }

    #[test]
    fn nested_vector_vector_u8() {
        // vector<vector<u8>> -> Uint8Array[]
        let ty = MoveType::Vector(Box::new(MoveType::Vector(Box::new(MoveType::U8))));
        assert_eq!(to_ts_type(&ty), "Uint8Array[]");
    }

    // ---- VecMap type mapping tests ----

    #[test]
    fn vecmap_maps_to_ts_map() {
        let ty = MoveType::VecMap(Box::new(MoveType::U64), Box::new(MoveType::Bool));
        assert_eq!(to_ts_type(&ty), "Map<bigint, boolean>");
    }

    #[test]
    fn vecmap_address_string() {
        let ty = MoveType::VecMap(Box::new(MoveType::Address), Box::new(MoveType::SuiString));
        assert_eq!(to_ts_type(&ty), "Map<string, string>");
    }

    #[test]
    fn vector_vecmap() {
        let ty = MoveType::Vector(Box::new(MoveType::VecMap(
            Box::new(MoveType::U64),
            Box::new(MoveType::Bool),
        )));
        assert_eq!(to_ts_type(&ty), "Map<bigint, boolean>[]");
    }

    #[test]
    fn option_vecmap() {
        let ty = MoveType::Option(Box::new(MoveType::VecMap(
            Box::new(MoveType::Address),
            Box::new(MoveType::U64),
        )));
        assert_eq!(to_ts_type(&ty), "Map<string, bigint> | null");
    }

    #[test]
    fn object_ref_maps_to_transaction_object_input() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "Pool".to_string(),
                type_args: vec![],
            }),
            is_mut: true,
        };
        assert_eq!(to_ts_type(&ty), "TransactionObjectInput");
    }

    #[test]
    fn type_param_without_key_maps_to_string() {
        let ty = MoveType::TypeParam {
            name: "T".to_string(),
            has_key: false,
        };
        assert_eq!(to_ts_type(&ty), "string");
    }

    #[test]
    fn type_param_with_key_maps_to_transaction_object_input() {
        // T: key + store should use TransactionObjectInput (it's an object)
        let ty = MoveType::TypeParam {
            name: "T".to_string(),
            has_key: true,
        };
        assert_eq!(to_ts_type(&ty), "TransactionObjectInput");
    }

    #[test]
    fn type_param_with_key_uses_tx_object() {
        let ty = MoveType::TypeParam {
            name: "T".to_string(),
            has_key: true,
        };
        assert_eq!(to_tx_encoding(&ty, "args.nft"), "tx.object(args.nft)");
    }

    #[test]
    fn type_param_without_key_uses_tx_pure() {
        let ty = MoveType::TypeParam {
            name: "T".to_string(),
            has_key: false,
        };
        assert_eq!(to_tx_encoding(&ty, "args.value"), "tx.pure(args.value)");
    }

    // ---- to_tx_encoding tests ----

    #[test]
    fn u64_encoding() {
        assert_eq!(
            to_tx_encoding(&MoveType::U64, "args.price"),
            "tx.pure.u64(args.price)"
        );
    }

    #[test]
    fn bool_encoding() {
        assert_eq!(
            to_tx_encoding(&MoveType::Bool, "args.active"),
            "tx.pure.bool(args.active)"
        );
    }

    #[test]
    fn address_encoding() {
        assert_eq!(
            to_tx_encoding(&MoveType::Address, "args.recipient"),
            "tx.pure.address(args.recipient)"
        );
    }

    #[test]
    fn string_encoding() {
        assert_eq!(
            to_tx_encoding(&MoveType::SuiString, "args.name"),
            "tx.pure.string(args.name)"
        );
    }

    #[test]
    fn object_id_encoding() {
        assert_eq!(
            to_tx_encoding(&MoveType::ObjectId, "args.id"),
            "tx.pure.id(args.id)"
        );
    }

    #[test]
    fn vector_u8_encoding() {
        let ty = MoveType::Vector(Box::new(MoveType::U8));
        assert_eq!(
            to_tx_encoding(&ty, "args.data"),
            "tx.pure('vector<u8>', args.data)"
        );
    }

    #[test]
    fn vector_u64_encoding() {
        let ty = MoveType::Vector(Box::new(MoveType::U64));
        assert_eq!(
            to_tx_encoding(&ty, "args.amounts"),
            "tx.pure.vector('u64', args.amounts)"
        );
    }

    #[test]
    fn option_u64_encoding() {
        let ty = MoveType::Option(Box::new(MoveType::U64));
        assert_eq!(
            to_tx_encoding(&ty, "args.limit"),
            "tx.pure.option('u64', args.limit)"
        );
    }

    #[test]
    fn option_vector_u8_encoding() {
        let ty = MoveType::Option(Box::new(MoveType::Vector(Box::new(MoveType::U8))));
        assert_eq!(
            to_tx_encoding(&ty, "args.data"),
            "tx.pure.option('vector<u8>', args.data)"
        );
    }

    #[test]
    fn object_ref_encoding() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "Pool".to_string(),
                type_args: vec![],
            }),
            is_mut: true,
        };
        assert_eq!(to_tx_encoding(&ty, "args.poolId"), "tx.object(args.poolId)");
    }

    // ---- name conversion tests ----

    #[test]
    fn snake_to_camel() {
        assert_eq!(to_camel_case("list_item"), "listItem");
        assert_eq!(to_camel_case("cancel_listing"), "cancelListing");
        assert_eq!(to_camel_case("get_price"), "getPrice");
        assert_eq!(to_camel_case("withdraw"), "withdraw");
    }

    #[test]
    fn name_to_env_var() {
        assert_eq!(to_env_var_name("my_dex"), "MY_DEX");
        assert_eq!(to_env_var_name("marketplace"), "MARKETPLACE");
        assert_eq!(to_env_var_name("MyProject"), "MY_PROJECT");
    }

    // ---- BCS type string tests ----

    #[test]
    fn bcs_type_strings() {
        assert_eq!(to_bcs_type_string(&MoveType::U8), "u8");
        assert_eq!(to_bcs_type_string(&MoveType::U64), "u64");
        assert_eq!(to_bcs_type_string(&MoveType::Bool), "bool");
        assert_eq!(to_bcs_type_string(&MoveType::Address), "address");
        assert_eq!(
            to_bcs_type_string(&MoveType::Vector(Box::new(MoveType::U64))),
            "vector<u64>"
        );
        assert_eq!(
            to_bcs_type_string(&MoveType::Option(Box::new(MoveType::Address))),
            "option<address>"
        );
        assert_eq!(
            to_bcs_type_string(&MoveType::Vector(Box::new(MoveType::Option(Box::new(
                MoveType::U64
            ))))),
            "vector<option<u64>>"
        );
    }

    // ---- Code generation tests ----

    #[test]
    fn generates_errors_module() {
        let output = generate_errors_module();
        assert!(output.contains("export class InvalidConfigError extends Error"));
        assert!(!output.contains("validateSuiAddress")); // removed — uses @mysten/sui/utils
    }

    #[test]
    fn generates_simple_module() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "list_item".to_string(),
                is_entry: true,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "price".to_string(),
                    move_type: MoveType::U64,
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("import process from 'node:process';"));
        assert!(output.contains("import { isValidSuiAddress } from '@mysten/sui/utils';"));
        assert!(output.contains("function getPackageId(): string {"));
        assert!(output.contains("isValidSuiAddress(id)"));
        assert!(output.contains("MY_PROJECT_PACKAGE_ID"));
        assert!(output.contains("export function listItem("));
        assert!(output.contains("price: bigint;"));
        assert!(output.contains("tx.pure.u64(args.price)"));
        assert!(output.contains("marketplace::list_item"));
    }

    #[test]
    fn generates_singleton_getter() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "get_price".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "marketplace".to_string(),
                    move_type: MoveType::Ref {
                        inner: Box::new(MoveType::Struct {
                            module: None,
                            name: "Marketplace".to_string(),
                            type_args: vec![],
                        }),
                        is_mut: false,
                    },
                    is_singleton: true,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::from(["Marketplace".to_string()]),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("function getMarketplaceId(): string {"));
        assert!(output.contains("MY_PROJECT_MARKETPLACE"));
        assert!(output.contains("marketplace?: TransactionObjectInput;"));
        assert!(output.contains("getMarketplaceId()"));
    }

    #[test]
    fn generates_generic_function() {
        let module = ModuleInfo {
            name: "pool".to_string(),
            functions: vec![FunctionInfo {
                name: "withdraw".to_string(),
                is_entry: false,
                type_params: vec![TypeParamInfo {
                    name: "T".to_string(),
                    has_key: false,
                }],
                params: vec![
                    ParamInfo {
                        name: "pool_id".to_string(),
                        move_type: MoveType::Ref {
                            inner: Box::new(MoveType::Struct {
                                module: None,
                                name: "Pool".to_string(),
                                type_args: vec![MoveType::TypeParam {
                                    name: "T".to_string(),
                                    has_key: false,
                                }],
                            }),
                            is_mut: true,
                        },
                        is_singleton: false,
                    },
                    ParamInfo {
                        name: "amount".to_string(),
                        move_type: MoveType::U64,
                        is_singleton: false,
                    },
                ],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("typeT: string;"));
        assert!(output.contains("typeArguments: [args.typeT]"));
    }

    #[test]
    fn generates_clock_injection() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "get_timed_price".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "marketplace".to_string(),
                    move_type: MoveType::Ref {
                        inner: Box::new(MoveType::Struct {
                            module: None,
                            name: "Marketplace".to_string(),
                            type_args: vec![],
                        }),
                        is_mut: false,
                    },
                    is_singleton: false,
                }],
                has_clock_param: true,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("tx.object.clock()"));
    }

    #[test]
    fn generates_struct_interface() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "create_listing".to_string(),
                is_entry: true,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "data".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "ListingData".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "ListingData".to_string(),
                fields: vec![
                    ("price".to_string(), MoveType::U64),
                    ("seller".to_string(), MoveType::Address),
                ],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("export interface ListingData {"));
        assert!(output.contains("price: bigint;"));
        assert!(output.contains("seller: string;"));
    }

    #[test]
    fn unreferenced_struct_not_emitted() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "get_count".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "value".to_string(),
                    move_type: MoveType::U64,
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "InternalState".to_string(),
                fields: vec![("count".to_string(), MoveType::U64)],
                has_key: false,
                has_copy: false,
                has_drop: false,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(!output.contains("export interface InternalState"));
    }

    #[test]
    fn generates_no_arg_function() {
        let module = ModuleInfo {
            name: "counter".to_string(),
            functions: vec![FunctionInfo {
                name: "reset".to_string(),
                is_entry: true,
                type_params: vec![],
                params: vec![],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("export function reset(tx: Transaction): TransactionResult {"));
    }

    // ---- Ref struct interface exclusion tests ----

    #[test]
    fn ref_struct_does_not_get_interface() {
        // A key struct passed by &mut should NOT generate export interface
        // because it maps to TransactionObjectInput (string object ID)
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "do_thing".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "store".to_string(),
                    move_type: MoveType::Ref {
                        inner: Box::new(MoveType::Struct {
                            module: None,
                            name: "Store".to_string(),
                            type_args: vec![],
                        }),
                        is_mut: true,
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Store".to_string(),
                fields: vec![
                    ("admin".to_string(), MoveType::Address),
                    ("fee_bps".to_string(), MoveType::U64),
                ],
                has_key: true,
                has_copy: false,
                has_drop: false,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            !output.contains("export interface Store"),
            "key struct passed by ref should NOT get an interface"
        );
        assert!(
            output.contains("store: TransactionObjectInput"),
            "ref param should be TransactionObjectInput"
        );
    }

    #[test]
    fn singleton_ref_struct_does_not_get_interface() {
        // A singleton key struct should NOT generate export interface
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "get_price".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "store".to_string(),
                    move_type: MoveType::Ref {
                        inner: Box::new(MoveType::Struct {
                            module: None,
                            name: "Store".to_string(),
                            type_args: vec![],
                        }),
                        is_mut: false,
                    },
                    is_singleton: true,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Store".to_string(),
                fields: vec![("fee".to_string(), MoveType::U64)],
                has_key: true,
                has_copy: false,
                has_drop: false,
            }],
            singletons: HashSet::from(["Store".to_string()]),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            !output.contains("export interface Store"),
            "singleton key struct should NOT get an interface"
        );
        assert!(
            output.contains("store?: TransactionObjectInput"),
            "singleton param should be optional TransactionObjectInput"
        );
    }

    #[test]
    fn by_value_struct_gets_interface_but_ref_does_not() {
        // Module with two structs: Config (copy+drop, by value) and Store (key, by ref)
        // Only Config should get an interface
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "update".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![
                    ParamInfo {
                        name: "store".to_string(),
                        move_type: MoveType::Ref {
                            inner: Box::new(MoveType::Struct {
                                module: None,
                                name: "Store".to_string(),
                                type_args: vec![],
                            }),
                            is_mut: true,
                        },
                        is_singleton: false,
                    },
                    ParamInfo {
                        name: "config".to_string(),
                        move_type: MoveType::Struct {
                            module: None,
                            name: "Config".to_string(),
                            type_args: vec![],
                        },
                        is_singleton: false,
                    },
                ],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![
                StructInfo {
                    name: "Store".to_string(),
                    fields: vec![("fee".to_string(), MoveType::U64)],
                    has_key: true,
                    has_copy: false,
                    has_drop: false,
                },
                StructInfo {
                    name: "Config".to_string(),
                    fields: vec![
                        ("max_items".to_string(), MoveType::U64),
                        ("enabled".to_string(), MoveType::Bool),
                    ],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
            ],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            !output.contains("export interface Store"),
            "Store (key, by ref) should NOT get interface"
        );
        assert!(
            output.contains("export interface Config {"),
            "Config (copy+drop, by value) SHOULD get interface"
        );
        assert!(output.contains("maxItems: bigint;"));
        assert!(output.contains("enabled: boolean;"));
    }

    #[test]
    fn external_object_struct_maps_to_transaction_object_input() {
        // Coin<USDC>, Kiosk, etc. are external structs not in our module.
        // They should map to TransactionObjectInput, not their struct name.
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "buy_item".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![
                    ParamInfo {
                        name: "coin".to_string(),
                        move_type: MoveType::Struct {
                            module: Some("coin".to_string()),
                            name: "Coin".to_string(),
                            type_args: vec![MoveType::Struct {
                                module: Some("usdc".to_string()),
                                name: "USDC".to_string(),
                                type_args: vec![],
                            }],
                        },
                        is_singleton: false,
                    },
                    ParamInfo {
                        name: "kiosk".to_string(),
                        move_type: MoveType::Struct {
                            module: Some("kiosk".to_string()),
                            name: "Kiosk".to_string(),
                            type_args: vec![],
                        },
                        is_singleton: false,
                    },
                ],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![], // no own structs — Coin and Kiosk are external
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("coin: TransactionObjectInput;"),
            "Coin<USDC> should be TransactionObjectInput, got: {}",
            output
        );
        assert!(
            output.contains("kiosk: TransactionObjectInput;"),
            "Kiosk should be TransactionObjectInput"
        );
        // Should NOT contain raw type names as types
        assert!(!output.contains("coin: Coin;"));
        assert!(!output.contains("kiosk: Kiosk;"));
    }

    // ---- BCS serialization tests ----

    #[test]
    fn bcs_schema_primitives() {
        assert_eq!(to_bcs_schema(&MoveType::U8, &[]), "bcs.u8()");
        assert_eq!(to_bcs_schema(&MoveType::U64, &[]), "bcs.u64()");
        assert_eq!(to_bcs_schema(&MoveType::Bool, &[]), "bcs.bool()");
        assert_eq!(to_bcs_schema(&MoveType::Address, &[]), "bcs.Address");
        assert_eq!(to_bcs_schema(&MoveType::SuiString, &[]), "bcs.string()");
        assert_eq!(to_bcs_schema(&MoveType::ObjectId, &[]), "bcs.Address");
    }

    #[test]
    fn type_contains_address_recursion() {
        assert!(type_contains_address(&MoveType::Address, &[]));
        assert!(type_contains_address(&MoveType::ObjectId, &[]));
        assert!(type_contains_address(
            &MoveType::Vector(Box::new(MoveType::Address)),
            &[]
        ));
        assert!(type_contains_address(
            &MoveType::Option(Box::new(MoveType::ObjectId)),
            &[]
        ));
        assert!(type_contains_address(
            &MoveType::Vector(Box::new(MoveType::Option(Box::new(MoveType::Address)))),
            &[]
        ));
        assert!(!type_contains_address(&MoveType::U64, &[]));
        assert!(!type_contains_address(
            &MoveType::Vector(Box::new(MoveType::U64)),
            &[]
        ));
        // Unknown struct (not in module) returns false
        assert!(!type_contains_address(
            &MoveType::Struct {
                module: None,
                name: "Foo".to_string(),
                type_args: vec![],
            },
            &[]
        ));
    }

    #[test]
    fn bcs_schema_nested() {
        let vec_u64 = MoveType::Vector(Box::new(MoveType::U64));
        assert_eq!(to_bcs_schema(&vec_u64, &[]), "bcs.vector(bcs.u64())");

        let opt_bool = MoveType::Option(Box::new(MoveType::Bool));
        assert_eq!(to_bcs_schema(&opt_bool, &[]), "bcs.option(bcs.bool())");

        let vec_opt_u8 = MoveType::Vector(Box::new(MoveType::Option(Box::new(MoveType::U8))));
        assert_eq!(
            to_bcs_schema(&vec_opt_u8, &[]),
            "bcs.vector(bcs.option(bcs.u8()))"
        );
    }

    #[test]
    fn bcs_schema_vecmap() {
        let ty = MoveType::VecMap(Box::new(MoveType::U64), Box::new(MoveType::Bool));
        assert_eq!(to_bcs_schema(&ty, &[]), "bcs.map(bcs.u64(), bcs.bool())");
    }

    #[test]
    fn bcs_schema_vecmap_with_address() {
        let ty = MoveType::VecMap(Box::new(MoveType::Address), Box::new(MoveType::SuiString));
        assert_eq!(
            to_bcs_schema(&ty, &[]),
            "bcs.map(bcs.Address, bcs.string())"
        );
    }

    #[test]
    fn bcs_schema_vector_of_vecmap() {
        let ty = MoveType::Vector(Box::new(MoveType::VecMap(
            Box::new(MoveType::U64),
            Box::new(MoveType::Bool),
        )));
        assert_eq!(
            to_bcs_schema(&ty, &[]),
            "bcs.vector(bcs.map(bcs.u64(), bcs.bool()))"
        );
    }

    #[test]
    fn bcs_schema_option_of_vecmap() {
        let ty = MoveType::Option(Box::new(MoveType::VecMap(
            Box::new(MoveType::Address),
            Box::new(MoveType::U64),
        )));
        assert_eq!(
            to_bcs_schema(&ty, &[]),
            "bcs.option(bcs.map(bcs.Address, bcs.u64()))"
        );
    }

    #[test]
    fn vecmap_tx_encoding() {
        let ty = MoveType::VecMap(Box::new(MoveType::U64), Box::new(MoveType::Bool));
        let result = to_tx_encoding(&ty, "args.data");
        assert_eq!(
            result,
            "tx.pure(bcs.map(bcs.u64(), bcs.bool()).serialize(args.data))"
        );
    }

    #[test]
    fn vector_vecmap_tx_encoding() {
        let ty = MoveType::Vector(Box::new(MoveType::VecMap(
            Box::new(MoveType::U64),
            Box::new(MoveType::Bool),
        )));
        let result = to_tx_encoding(&ty, "args.items");
        assert_eq!(
            result,
            "tx.pure(bcs.vector(bcs.map(bcs.u64(), bcs.bool())).serialize(args.items))"
        );
    }

    #[test]
    fn option_vecmap_tx_encoding() {
        let ty = MoveType::Option(Box::new(MoveType::VecMap(
            Box::new(MoveType::Address),
            Box::new(MoveType::U64),
        )));
        let result = to_tx_encoding(&ty, "args.opt");
        assert_eq!(
            result,
            "tx.pure(bcs.option(bcs.map(bcs.Address, bcs.u64())).serialize(args.opt))"
        );
    }

    #[test]
    fn vector_without_vecmap_uses_type_string() {
        // Simple Vector<u64> should still use tx.pure.vector, not bcs.vector
        let ty = MoveType::Vector(Box::new(MoveType::U64));
        let result = to_tx_encoding(&ty, "args.v");
        assert_eq!(result, "tx.pure.vector('u64', args.v)");
    }

    #[test]
    fn bcs_struct_encoding() {
        let si = StructInfo {
            name: "MyData".to_string(),
            fields: vec![
                ("value".to_string(), MoveType::U64),
                ("flag".to_string(), MoveType::Bool),
                ("name".to_string(), MoveType::SuiString),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let result = to_bcs_struct_encoding(&si, "args.data", &[]);
        assert_eq!(
            result,
            "tx.pure(bcs.struct('MyData', { value: bcs.u64(), flag: bcs.bool(), name: bcs.string() }).serialize(args.data))"
        );
    }

    #[test]
    fn bcs_schema_expands_nested_pure_struct() {
        let inner = StructInfo {
            name: "Inner".to_string(),
            fields: vec![("value".to_string(), MoveType::U64)],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let structs = vec![inner];
        let ty = MoveType::Struct {
            module: None,
            name: "Inner".to_string(),
            type_args: vec![],
        };
        assert_eq!(
            to_bcs_schema(&ty, &structs),
            "bcs.struct('Inner', { value: bcs.u64() })"
        );
    }

    #[test]
    fn bcs_schema_falls_back_for_non_pure_struct() {
        let key_struct = StructInfo {
            name: "Pool".to_string(),
            fields: vec![("balance".to_string(), MoveType::U64)],
            has_key: true,
            has_copy: false,
            has_drop: false,
        };
        let structs = vec![key_struct];
        let ty = MoveType::Struct {
            module: None,
            name: "Pool".to_string(),
            type_args: vec![],
        };
        assert_eq!(to_bcs_schema(&ty, &structs), "bcs.bytes(32)");
    }

    #[test]
    fn bcs_struct_encoding_with_nested_struct() {
        let inner = StructInfo {
            name: "Inner".to_string(),
            fields: vec![
                ("value".to_string(), MoveType::U64),
                ("flag".to_string(), MoveType::Bool),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let outer = StructInfo {
            name: "Outer".to_string(),
            fields: vec![
                (
                    "inner".to_string(),
                    MoveType::Struct {
                        module: None,
                        name: "Inner".to_string(),
                        type_args: vec![],
                    },
                ),
                ("count".to_string(), MoveType::U64),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let structs = vec![inner, outer.clone()];
        let result = to_bcs_struct_encoding(&outer, "args.outer", &structs);
        assert_eq!(
            result,
            "tx.pure(bcs.struct('Outer', { inner: bcs.struct('Inner', { value: bcs.u64(), flag: bcs.bool() }), count: bcs.u64() }).serialize(args.outer))"
        );
    }

    #[test]
    fn bcs_struct_encoding_with_vector_of_nested_struct() {
        let inner = StructInfo {
            name: "Item".to_string(),
            fields: vec![
                ("price".to_string(), MoveType::U64),
                ("name".to_string(), MoveType::SuiString),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let outer = StructInfo {
            name: "Order".to_string(),
            fields: vec![
                (
                    "items".to_string(),
                    MoveType::Vector(Box::new(MoveType::Struct {
                        module: None,
                        name: "Item".to_string(),
                        type_args: vec![],
                    })),
                ),
                ("total".to_string(), MoveType::U64),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let structs = vec![inner, outer.clone()];
        let result = to_bcs_struct_encoding(&outer, "args.order", &structs);
        assert_eq!(
            result,
            "tx.pure(bcs.struct('Order', { items: bcs.vector(bcs.struct('Item', { price: bcs.u64(), name: bcs.string() })), total: bcs.u64() }).serialize(args.order))"
        );
    }

    #[test]
    fn bcs_struct_encoding_with_option_of_nested_struct() {
        let inner = StructInfo {
            name: "Metadata".to_string(),
            fields: vec![("label".to_string(), MoveType::SuiString)],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let outer = StructInfo {
            name: "Entry".to_string(),
            fields: vec![
                ("value".to_string(), MoveType::U64),
                (
                    "metadata".to_string(),
                    MoveType::Option(Box::new(MoveType::Struct {
                        module: None,
                        name: "Metadata".to_string(),
                        type_args: vec![],
                    })),
                ),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let structs = vec![inner, outer.clone()];
        let result = to_bcs_struct_encoding(&outer, "args.entry", &structs);
        assert_eq!(
            result,
            "tx.pure(bcs.struct('Entry', { value: bcs.u64(), metadata: bcs.option(bcs.struct('Metadata', { label: bcs.string() })) }).serialize(args.entry))"
        );
    }

    #[test]
    fn type_contains_address_through_vector_of_struct() {
        let inner = StructInfo {
            name: "Target".to_string(),
            fields: vec![("addr".to_string(), MoveType::Address)],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let structs = vec![inner];

        // Vector<Target> where Target has an address field
        assert!(type_contains_address(
            &MoveType::Vector(Box::new(MoveType::Struct {
                module: None,
                name: "Target".to_string(),
                type_args: vec![],
            })),
            &structs
        ));

        // Option<Target>
        assert!(type_contains_address(
            &MoveType::Option(Box::new(MoveType::Struct {
                module: None,
                name: "Target".to_string(),
                type_args: vec![],
            })),
            &structs
        ));
    }

    #[test]
    fn type_contains_address_recurses_into_nested_struct() {
        let inner_with_id = StructInfo {
            name: "Inner".to_string(),
            fields: vec![("target".to_string(), MoveType::ObjectId)],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        let inner_no_id = StructInfo {
            name: "Plain".to_string(),
            fields: vec![("value".to_string(), MoveType::U64)],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };

        let structs = vec![inner_with_id, inner_no_id];

        // Struct with address field inside
        assert!(type_contains_address(
            &MoveType::Struct {
                module: None,
                name: "Inner".to_string(),
                type_args: vec![],
            },
            &structs
        ));

        // Struct without address field
        assert!(!type_contains_address(
            &MoveType::Struct {
                module: None,
                name: "Plain".to_string(),
                type_args: vec![],
            },
            &structs
        ));
    }

    #[test]
    fn pure_value_struct_uses_bcs_not_object() {
        let pure_struct = StructInfo {
            name: "Config".to_string(),
            fields: vec![
                ("max_size".to_string(), MoveType::U64),
                ("enabled".to_string(), MoveType::Bool),
            ],
            has_key: false,
            has_copy: true,
            has_drop: true,
        };
        assert!(pure_struct.is_pure_value());

        let structs = vec![pure_struct];
        let ty = MoveType::Struct {
            module: None,
            name: "Config".to_string(),
            type_args: vec![],
        };

        let result = to_tx_encoding_with_context(&ty, "args.config", &structs);
        assert!(result.contains("bcs.struct('Config'"));
        assert!(result.contains("bcs.u64()"));
        assert!(result.contains("bcs.bool()"));
        assert!(!result.contains("tx.object"));
    }

    #[test]
    fn key_struct_uses_object_not_bcs() {
        let key_struct = StructInfo {
            name: "Pool".to_string(),
            fields: vec![("balance".to_string(), MoveType::U64)],
            has_key: true,
            has_copy: false,
            has_drop: false,
        };
        assert!(!key_struct.is_pure_value());

        let structs = vec![key_struct];
        let ty = MoveType::Struct {
            module: None,
            name: "Pool".to_string(),
            type_args: vec![],
        };

        let result = to_tx_encoding_with_context(&ty, "args.poolId", &structs);
        assert_eq!(result, "tx.object(args.poolId)");
    }

    #[test]
    fn unknown_struct_defaults_to_object() {
        let structs: Vec<StructInfo> = vec![];
        let ty = MoveType::Struct {
            module: Some("other".to_string()),
            name: "ExternalType".to_string(),
            type_args: vec![],
        };

        let result = to_tx_encoding_with_context(&ty, "args.ext", &structs);
        assert_eq!(result, "tx.object(args.ext)");
    }

    #[test]
    fn generates_bcs_import_when_pure_struct_used() {
        let module = ModuleInfo {
            name: "config_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "set_config".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "config".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "Config".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Config".to_string(),
                fields: vec![("max_size".to_string(), MoveType::U64)],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/bcs';"),
            "should import bcs"
        );
        assert!(
            output.contains("bcs.struct('Config'"),
            "should use BCS struct encoding"
        );
        assert!(
            !output.contains("tx.object(args.config)"),
            "should NOT use tx.object for pure struct"
        );
    }

    #[test]
    fn no_bcs_import_when_only_key_structs() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "do_thing".to_string(),
                is_entry: true,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "pool".to_string(),
                    move_type: MoveType::Ref {
                        inner: Box::new(MoveType::Struct {
                            module: None,
                            name: "Pool".to_string(),
                            type_args: vec![],
                        }),
                        is_mut: true,
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Pool".to_string(),
                fields: vec![],
                has_key: true,
                has_copy: false,
                has_drop: false,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            !output.contains("@mysten/bcs"),
            "should NOT import bcs when no pure structs"
        );
    }

    #[test]
    fn imports_address_when_pure_struct_has_id_field() {
        let module = ModuleInfo {
            name: "receipt_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "submit_receipt".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "receipt".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "Receipt".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Receipt".to_string(),
                fields: vec![
                    ("item_id".to_string(), MoveType::ObjectId),
                    ("amount".to_string(), MoveType::U64),
                ],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/sui/bcs';"),
            "should import bcs from @mysten/sui/bcs when Address is needed"
        );
        assert!(
            output.contains("itemId: bcs.Address"),
            "should use bcs.Address in struct schema"
        );
    }

    #[test]
    fn no_address_import_when_pure_struct_has_no_id() {
        let module = ModuleInfo {
            name: "config_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "set_config".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "config".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "Config".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "Config".to_string(),
                fields: vec![("max_size".to_string(), MoveType::U64)],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            !output.contains("@mysten/sui/bcs"),
            "should NOT import Address when no Address/ObjectId fields"
        );
    }

    #[test]
    fn vecmap_param_imports_bcs() {
        let module = ModuleInfo {
            name: "map_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "set_config".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "data".to_string(),
                    move_type: MoveType::VecMap(Box::new(MoveType::U64), Box::new(MoveType::Bool)),
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/bcs';"),
            "VecMap param should trigger bcs import"
        );
        assert!(
            output.contains("bcs.map(bcs.u64(), bcs.bool())"),
            "should use bcs.map encoding"
        );
        assert!(
            output.contains("data: Map<bigint, boolean>"),
            "should type param as Map<bigint, boolean>"
        );
    }

    #[test]
    fn vecmap_address_param_imports_address() {
        let module = ModuleInfo {
            name: "map_addr_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "set_targets".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "targets".to_string(),
                    move_type: MoveType::VecMap(
                        Box::new(MoveType::Address),
                        Box::new(MoveType::U64),
                    ),
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/sui/bcs';"),
            "VecMap<address, u64> should import bcs from @mysten/sui/bcs"
        );
        assert!(
            output.contains("bcs.map(bcs.Address, bcs.u64())"),
            "should use bcs.Address in bcs.map"
        );
    }

    #[test]
    fn vecmap_no_address_import_without_address_types() {
        let module = ModuleInfo {
            name: "map_no_addr".to_string(),
            functions: vec![FunctionInfo {
                name: "set_flags".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "flags".to_string(),
                    move_type: MoveType::VecMap(
                        Box::new(MoveType::SuiString),
                        Box::new(MoveType::Bool),
                    ),
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/bcs';"),
            "should import bcs"
        );
        assert!(
            !output.contains("@mysten/sui/bcs"),
            "should NOT import Address when VecMap has no address types"
        );
    }

    #[test]
    fn nested_pure_struct_generates_nested_bcs() {
        let module = ModuleInfo {
            name: "nested_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "submit".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "outer".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "Outer".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![
                StructInfo {
                    name: "Inner".to_string(),
                    fields: vec![("value".to_string(), MoveType::U64)],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
                StructInfo {
                    name: "Outer".to_string(),
                    fields: vec![
                        (
                            "inner".to_string(),
                            MoveType::Struct {
                                module: None,
                                name: "Inner".to_string(),
                                type_args: vec![],
                            },
                        ),
                        ("count".to_string(), MoveType::U64),
                    ],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
            ],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("bcs.struct('Inner', { value: bcs.u64() })"),
            "should expand Inner struct inline in BCS schema, got:\n{output}"
        );
        assert!(
            output.contains("bcs.struct('Outer'"),
            "should have Outer struct in BCS schema"
        );
    }

    #[test]
    fn imports_address_for_nested_struct_with_id() {
        let module = ModuleInfo {
            name: "nested_addr_mod".to_string(),
            functions: vec![FunctionInfo {
                name: "submit".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "outer".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "Outer".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![
                StructInfo {
                    name: "Inner".to_string(),
                    fields: vec![("target".to_string(), MoveType::ObjectId)],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
                StructInfo {
                    name: "Outer".to_string(),
                    fields: vec![
                        (
                            "inner".to_string(),
                            MoveType::Struct {
                                module: None,
                                name: "Inner".to_string(),
                                type_args: vec![],
                            },
                        ),
                        ("count".to_string(), MoveType::U64),
                    ],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
            ],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(
            output.contains("import { bcs } from '@mysten/sui/bcs';"),
            "should import bcs from @mysten/sui/bcs when nested struct has ObjectId field"
        );
        assert!(
            output.contains("target: bcs.Address"),
            "nested Inner struct should use bcs.Address in BCS schema"
        );
    }

    // ---- Event type generation tests ----

    #[test]
    fn generates_event_types_when_enabled() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![],
            structs: vec![StructInfo {
                name: "ItemPurchased".to_string(),
                fields: vec![
                    ("buyer".to_string(), MoveType::Address),
                    ("price".to_string(), MoveType::U64),
                    ("item_id".to_string(), MoveType::Address),
                ],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::from(["ItemPurchased".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("export type ItemPurchased = {"));
        assert!(output.contains("export type ItemPurchased = {"));
        assert!(output.contains("readonly buyer: string;"));
        assert!(output.contains("readonly price: string;"));
        assert!(output.contains("readonly item_id: string;"));
    }

    #[test]
    fn no_event_types_when_disabled() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![],
            structs: vec![StructInfo {
                name: "ItemPurchased".to_string(),
                fields: vec![("buyer".to_string(), MoveType::Address)],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::from(["ItemPurchased".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: false,
        };

        let output = generate_module(&module, &config);
        assert!(!output.contains("Event Types"));
        assert!(!output.contains("export type ItemPurchased"));
    }

    #[test]
    fn event_not_emitted_is_excluded() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "search".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "range".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "PriceRange".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![
                StructInfo {
                    name: "PriceRange".to_string(),
                    fields: vec![("min_price".to_string(), MoveType::U64)],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
                StructInfo {
                    name: "ItemPurchased".to_string(),
                    fields: vec![("buyer".to_string(), MoveType::Address)],
                    has_key: false,
                    has_copy: true,
                    has_drop: true,
                },
            ],
            singletons: HashSet::new(),
            // Only ItemPurchased is emitted — PriceRange is NOT
            emitted_events: HashSet::from(["ItemPurchased".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("export type ItemPurchased = {"));
        // PriceRange is not emitted, so no event type for it
        assert!(!output.contains("export type PriceRange"));
    }

    #[test]
    fn event_used_as_param_gets_suffix() {
        // A struct that is BOTH emitted AND used as a function param
        // gets an "Event" suffix on the event type to avoid collision
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![FunctionInfo {
                name: "replay_event".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "event_data".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "TradeExecuted".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "TradeExecuted".to_string(),
                fields: vec![
                    ("buyer".to_string(), MoveType::Address),
                    ("amount".to_string(), MoveType::U64),
                ],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::from(["TradeExecuted".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        // Should have BCS interface (for function param)
        assert!(output.contains("export interface TradeExecuted {"));
        // Should have event type WITH Event suffix (for event consumption)
        assert!(output.contains("export type TradeExecutedEvent = {"));
        // Event fields should all be readonly string
        assert!(output.contains("readonly buyer: string;"));
        assert!(output.contains("readonly amount: string;"));
    }

    #[test]
    fn event_excludes_non_emitted_structs() {
        let module = ModuleInfo {
            name: "marketplace".to_string(),
            functions: vec![],
            structs: vec![StructInfo {
                name: "Marketplace".to_string(),
                fields: vec![],
                has_key: true,
                has_copy: false,
                has_drop: false,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::new(), // nothing emitted
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        assert!(!output.contains("export type Marketplace"));
        assert!(!output.contains("Event Types"));
    }

    #[test]
    fn event_suffix_always_added_on_collision() {
        // If struct name already ends with "Event" but collides with a param interface,
        // still add "Event" suffix — a collision is worse than an ugly name.
        let module = ModuleInfo {
            name: "trading".to_string(),
            functions: vec![FunctionInfo {
                name: "process".to_string(),
                is_entry: false,
                type_params: vec![],
                params: vec![ParamInfo {
                    name: "data".to_string(),
                    move_type: MoveType::Struct {
                        module: None,
                        name: "TradeEvent".to_string(),
                        type_args: vec![],
                    },
                    is_singleton: false,
                }],
                has_clock_param: false,
                has_random_param: false,
            }],
            structs: vec![StructInfo {
                name: "TradeEvent".to_string(),
                fields: vec![("amount".to_string(), MoveType::U64)],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::from(["TradeEvent".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        // Should be "TradeEventEvent" to avoid collision with the param interface
        assert!(output.contains("export type TradeEventEvent = {"));
    }

    #[test]
    fn event_fields_are_all_string_regardless_of_move_type() {
        // Event fields must ALL be string in TS, regardless of their Move type.
        // This test uses every possible Move type to verify none leak through.
        let module = ModuleInfo {
            name: "test_mod".to_string(),
            functions: vec![],
            structs: vec![StructInfo {
                name: "ComplexEvent".to_string(),
                fields: vec![
                    ("amount".to_string(), MoveType::U64),
                    ("small_val".to_string(), MoveType::U8),
                    ("big_val".to_string(), MoveType::U256),
                    ("is_active".to_string(), MoveType::Bool),
                    ("sender".to_string(), MoveType::Address),
                    ("name".to_string(), MoveType::SuiString),
                    ("obj_id".to_string(), MoveType::ObjectId),
                    ("data".to_string(), MoveType::Vector(Box::new(MoveType::U8))),
                    (
                        "scores".to_string(),
                        MoveType::Vector(Box::new(MoveType::U64)),
                    ),
                    (
                        "maybe_val".to_string(),
                        MoveType::Option(Box::new(MoveType::U64)),
                    ),
                ],
                has_key: false,
                has_copy: true,
                has_drop: true,
            }],
            singletons: HashSet::new(),
            emitted_events: HashSet::from(["ComplexEvent".to_string()]),
        };

        let config = CodegenConfig {
            package_id_env_var: "MY_PROJECT_PACKAGE_ID".to_string(),
            project_name: "my_project".to_string(),
            include_events: true,
        };

        let output = generate_module(&module, &config);
        assert!(output.contains("export type ComplexEvent = {"));

        // Extract just the event type block
        let event_start = output.find("export type ComplexEvent").unwrap();
        let event_end = output[event_start..].find("};").unwrap() + event_start + 2;
        let event_block = &output[event_start..event_end];

        // Every field line must be "readonly <name>: string;"
        for line in event_block.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("export type") || trimmed == "};" || trimmed.is_empty() {
                continue;
            }
            assert!(
                trimmed.starts_with("readonly ") && trimmed.ends_with(": string;"),
                "Event field should be readonly string but got: '{trimmed}'"
            );
        }

        // Verify NO Move-mapped types leaked through
        assert!(
            !event_block.contains("number"),
            "number should not appear in event type"
        );
        assert!(
            !event_block.contains("bigint"),
            "bigint should not appear in event type"
        );
        assert!(
            !event_block.contains("boolean"),
            "boolean should not appear in event type"
        );
        assert!(
            !event_block.contains("Uint8Array"),
            "Uint8Array should not appear in event type"
        );
        assert!(
            !event_block.contains("null"),
            "null should not appear in event type"
        );
    }
}
