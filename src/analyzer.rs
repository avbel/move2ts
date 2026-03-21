use std::collections::{HashMap, HashSet};

use move_compiler::parser::ast::{
    Ability_, Definition, Exp, Exp_, FunctionBody_, ModuleDefinition, ModuleMember,
    NameAccessChain, NameAccessChain_, Sequence, SequenceItem_, StructFields, Type, Type_,
    Visibility,
};

pub use crate::ir::*;

/// Determines which parameters to strip and which to auto-inject.
/// Returns (visible_params, has_clock, has_random).
pub fn process_params(params: Vec<ParamInfo>) -> (Vec<ParamInfo>, bool, bool) {
    let mut has_clock = false;
    let mut has_random = false;
    let mut visible = Vec::new();

    for param in params {
        if param.move_type.is_tx_context() {
            // stripped entirely
            continue;
        }
        if param.move_type.is_clock() {
            has_clock = true;
            continue;
        }
        if param.move_type.is_random() {
            has_random = true;
            continue;
        }
        visible.push(param);
    }

    (visible, has_clock, has_random)
}

/// Filters functions by include/exclude lists.
/// Returns only functions that pass the filter.
pub fn filter_functions(
    functions: Vec<FunctionInfo>,
    methods: &std::option::Option<Vec<String>>,
    skip_methods: &std::option::Option<Vec<String>>,
) -> (Vec<FunctionInfo>, Vec<String>) {
    let mut warnings = Vec::new();

    let filtered: Vec<FunctionInfo> = match (methods, skip_methods) {
        (Some(include), None) => {
            let include_set: HashSet<&str> = include.iter().map(|s| s.as_str()).collect();
            let result: Vec<FunctionInfo> = functions
                .into_iter()
                .filter(|f| include_set.contains(f.name.as_str()))
                .collect();

            let found: HashSet<&str> = result.iter().map(|f| f.name.as_str()).collect();
            for name in &include_set {
                if !found.contains(name) {
                    warnings.push(format!("--methods: no function named '{name}' found"));
                }
            }

            result
        }
        (None, Some(exclude)) => {
            let exclude_set: HashSet<&str> = exclude.iter().map(|s| s.as_str()).collect();
            functions
                .into_iter()
                .filter(|f| !exclude_set.contains(f.name.as_str()))
                .collect()
        }
        _ => functions,
    };

    (filtered, warnings)
}

// ============================================================================
// AST → IR Extraction
// ============================================================================

/// Converts a move-compiler `Type_` AST node into our `MoveType` IR.
/// `type_param_names` is the set of generic type parameter names in scope.
fn convert_type(ty: &Type, type_param_names: &HashSet<String>) -> MoveType {
    match &ty.value {
        Type_::Unit => MoveType::Unit,
        Type_::Ref(is_mut, inner) => MoveType::Ref {
            inner: Box::new(convert_type(inner, type_param_names)),
            is_mut: *is_mut,
        },
        Type_::Apply(name_access_chain) => convert_apply_type(name_access_chain, type_param_names),
        Type_::Multiple(_) => MoveType::Unit, // tuples not relevant for TS wrappers
        Type_::Fun(_, _) => MoveType::Unit,   // function types not relevant
        Type_::UnresolvedError => MoveType::Unit,
    }
}

/// Converts a `Type_::Apply(NameAccessChain)` into a `MoveType`.
fn convert_apply_type(
    chain: &move_compiler::parser::ast::NameAccessChain,
    type_param_names: &HashSet<String>,
) -> MoveType {
    match &chain.value {
        NameAccessChain_::Single(entry) => {
            let name = entry.name.value.as_str();
            let tyargs: Vec<MoveType> = entry
                .tyargs
                .as_ref()
                .map(|sp_tyargs| {
                    sp_tyargs
                        .value
                        .iter()
                        .map(|t| convert_type(t, type_param_names))
                        .collect()
                })
                .unwrap_or_default();

            convert_single_name(name, &tyargs, type_param_names)
        }
        NameAccessChain_::Path(name_path) => {
            // Qualified path like 0x1::string::String or 0x2::object::ID
            let entries = &name_path.entries;
            let root_name = match &name_path.root.name.value {
                move_compiler::parser::ast::LeadingNameAccess_::Name(n) => {
                    n.value.as_str().to_string()
                }
                move_compiler::parser::ast::LeadingNameAccess_::AnonymousAddress(addr) => {
                    format!("{addr}")
                }
                move_compiler::parser::ast::LeadingNameAccess_::GlobalAddress(n) => {
                    n.value.as_str().to_string()
                }
            };

            // Get the last entry's name (the actual type name)
            if let Some(last_entry) = entries.last() {
                let type_name = last_entry.name.value.as_str();
                let tyargs: Vec<MoveType> = last_entry
                    .tyargs
                    .as_ref()
                    .map(|sp_tyargs| {
                        sp_tyargs
                            .value
                            .iter()
                            .map(|t| convert_type(t, type_param_names))
                            .collect()
                    })
                    .unwrap_or_default();

                // Check for well-known stdlib types
                match type_name {
                    "String"
                        if root_name == "0x1"
                            || root_name == "std"
                            || (entries.len() >= 2
                                && entries[0].name.value.as_str() == "string") =>
                    {
                        return MoveType::SuiString;
                    }
                    "ID" if root_name == "0x2"
                        || root_name == "sui"
                        || (entries.len() >= 2 && entries[0].name.value.as_str() == "object") =>
                    {
                        return MoveType::ObjectId;
                    }
                    _ => {}
                }

                // Build module name from path segments before the last
                let module_name = if entries.len() >= 2 {
                    Some(entries[entries.len() - 2].name.value.as_str().to_string())
                } else if !root_name.starts_with("0x") {
                    Some(root_name.clone())
                } else {
                    // For paths like 0x2::object::ID, get the module from entries
                    entries.first().map(|e| e.name.value.as_str().to_string())
                };

                MoveType::Struct {
                    module: module_name,
                    name: type_name.to_string(),
                    type_args: tyargs,
                }
            } else {
                // Path with no entries — just a root name. Treat as single name.
                let root_str = root_name.as_str();
                let tyargs: Vec<MoveType> = name_path
                    .root
                    .tyargs
                    .as_ref()
                    .map(|sp_tyargs| {
                        sp_tyargs
                            .value
                            .iter()
                            .map(|t| convert_type(t, type_param_names))
                            .collect()
                    })
                    .unwrap_or_default();
                convert_single_name(root_str, &tyargs, type_param_names)
            }
        }
    }
}

/// Converts a single unqualified name (e.g., "u64", "vector", "bool", "MyStruct")
/// into a `MoveType`.
fn convert_single_name(
    name: &str,
    tyargs: &[MoveType],
    type_param_names: &HashSet<String>,
) -> MoveType {
    match name {
        "u8" => MoveType::U8,
        "u16" => MoveType::U16,
        "u32" => MoveType::U32,
        "u64" => MoveType::U64,
        "u128" => MoveType::U128,
        "u256" => MoveType::U256,
        "bool" => MoveType::Bool,
        "address" => MoveType::Address,
        "vector" => {
            let inner = tyargs.first().cloned().unwrap_or(MoveType::U8);
            MoveType::Vector(Box::new(inner))
        }
        "Option" => {
            let inner = tyargs.first().cloned().unwrap_or(MoveType::Unit);
            MoveType::Option(Box::new(inner))
        }
        "String" => MoveType::SuiString,
        "ID" => MoveType::ObjectId,
        _ => {
            // Check if it's a type parameter
            if type_param_names.contains(name) {
                MoveType::TypeParam(name.to_string())
            } else {
                MoveType::Struct {
                    module: None,
                    name: name.to_string(),
                    type_args: tyargs.to_vec(),
                }
            }
        }
    }
}

/// Generic AST expression walker. Calls `visitor` on every expression node, then recurses.
fn walk_exp(exp: &Exp, visitor: &mut impl FnMut(&Exp)) {
    visitor(exp);
    match &exp.value {
        Exp_::Pack(_, fields) => {
            for (_field, field_exp) in fields {
                walk_exp(field_exp, visitor);
            }
        }
        Exp_::Call(_, args) => {
            for arg in &args.value {
                walk_exp(arg, visitor);
            }
        }
        Exp_::Block(seq) => {
            walk_sequence(seq, visitor);
        }
        Exp_::IfElse(cond, then_exp, else_exp) => {
            walk_exp(cond, visitor);
            walk_exp(then_exp, visitor);
            if let Some(else_e) = else_exp {
                walk_exp(else_e, visitor);
            }
        }
        Exp_::While(cond, body) => {
            walk_exp(cond, visitor);
            walk_exp(body, visitor);
        }
        Exp_::Loop(body) => {
            walk_exp(body, visitor);
        }
        Exp_::Labeled(_, inner) => {
            walk_exp(inner, visitor);
        }
        Exp_::Assign(lhs, rhs) => {
            walk_exp(lhs, visitor);
            walk_exp(rhs, visitor);
        }
        Exp_::Return(_, Some(inner))
        | Exp_::Abort(Some(inner))
        | Exp_::Dereference(inner)
        | Exp_::UnaryExp(_, inner)
        | Exp_::Borrow(_, inner)
        | Exp_::Dot(inner, _, _)
        | Exp_::Cast(inner, _)
        | Exp_::Annotate(inner, _)
        | Exp_::Parens(inner)
        | Exp_::Move(_, inner)
        | Exp_::Copy(_, inner) => {
            walk_exp(inner, visitor);
        }
        Exp_::BinopExp(lhs, _, rhs) => {
            walk_exp(lhs, visitor);
            walk_exp(rhs, visitor);
        }
        Exp_::DotCall(inner, _, _, _, _, args) => {
            walk_exp(inner, visitor);
            for arg in &args.value {
                walk_exp(arg, visitor);
            }
        }
        Exp_::ExpList(exps) => {
            for e in exps {
                walk_exp(e, visitor);
            }
        }
        Exp_::Lambda(_, _, body) => {
            walk_exp(body, visitor);
        }
        Exp_::Vector(_, _, args) => {
            for arg in &args.value {
                walk_exp(arg, visitor);
            }
        }
        Exp_::Match(subject, arms) => {
            walk_exp(subject, visitor);
            for arm in &arms.value {
                if let Some(guard) = &arm.value.guard {
                    walk_exp(guard, visitor);
                }
                walk_exp(&arm.value.rhs, visitor);
            }
        }
        Exp_::Index(inner, args) => {
            walk_exp(inner, visitor);
            for arg in &args.value {
                walk_exp(arg, visitor);
            }
        }
        // Terminal nodes or nodes without sub-expressions we care about
        Exp_::Value(_)
        | Exp_::Name(_)
        | Exp_::Unit
        | Exp_::Break(_, _)
        | Exp_::Continue(_)
        | Exp_::Return(_, None)
        | Exp_::Abort(None)
        | Exp_::Spec(_)
        | Exp_::UnresolvedError
        | Exp_::DotUnresolved(_, _)
        | Exp_::Quant(_, _, _, _, _) => {}
    }
}

/// Walks a sequence (block body), calling `visitor` on every expression node.
fn walk_sequence(seq: &Sequence, visitor: &mut impl FnMut(&Exp)) {
    for item in &seq.1 {
        match &item.value {
            SequenceItem_::Seq(exp) => walk_exp(exp, visitor),
            SequenceItem_::Bind(_, _, exp) => walk_exp(exp, visitor),
            SequenceItem_::Declare(_, _) => {}
        }
    }
    if let Some(trailing_exp) = seq.3.as_ref() {
        walk_exp(trailing_exp, visitor);
    }
}

/// Builds a map of struct_name → set of function names that construct it.
/// Scans all function bodies in a module definition.
fn build_constructor_map(module_def: &ModuleDefinition) -> HashMap<String, HashSet<String>> {
    let mut constructor_map: HashMap<String, HashSet<String>> = HashMap::new();

    for member in &module_def.members {
        if let ModuleMember::Function(func) = member {
            let func_name = func.name.0.value.as_str().to_string();

            if let FunctionBody_::Defined(seq) = &func.body.value {
                let mut constructors = HashSet::new();
                walk_sequence(seq, &mut |e| {
                    if let Exp_::Pack(chain, _) = &e.value
                        && let Some(name) = extract_name_from_chain(chain)
                    {
                        constructors.insert(name);
                    }
                });

                for struct_name in constructors {
                    constructor_map
                        .entry(struct_name)
                        .or_default()
                        .insert(func_name.clone());
                }
            }
        }
    }

    constructor_map
}

/// Detects singletons: structs only constructed in `init()` that are on-chain objects (have `key`).
/// Pure value structs (copy+drop, no key) are excluded — they cannot be on-chain singletons.
fn detect_singletons(
    constructor_map: &HashMap<String, HashSet<String>>,
    structs: &[StructInfo],
) -> HashSet<String> {
    let mut singletons = HashSet::new();

    for (struct_name, constructing_fns) in constructor_map {
        if constructing_fns.len() == 1 && constructing_fns.contains("init") {
            // Only key-bearing structs can be on-chain singletons
            let is_object = structs
                .iter()
                .find(|s| s.name == *struct_name)
                .is_some_and(|s| s.has_key);
            if is_object {
                singletons.insert(struct_name.clone());
            }
        }
    }

    singletons
}

/// Detects event structs by scanning function bodies for `event::emit()` / `emit()` calls.
/// Returns the set of struct names that are emitted as events.
fn detect_emitted_events(module_def: &ModuleDefinition) -> HashSet<String> {
    let mut emitted = HashSet::new();

    for member in &module_def.members {
        if let ModuleMember::Function(func) = member
            && let FunctionBody_::Defined(seq) = &func.body.value
        {
            walk_sequence(seq, &mut |e| {
                if let Exp_::Call(chain, args) = &e.value
                    && is_emit_call(chain)
                {
                    for arg in &args.value {
                        collect_emitted_struct_name(arg, &mut emitted);
                    }
                }
            });
        }
    }

    emitted
}

/// Checks if a NameAccessChain refers to `emit` / `event::emit` / `sui::event::emit`.
fn is_emit_call(chain: &NameAccessChain) -> bool {
    match &chain.value {
        NameAccessChain_::Single(entry) => entry.name.value.as_str() == "emit",
        NameAccessChain_::Path(path) => {
            // Check last entry is "emit"
            if let Some(last) = path.entries.last()
                && last.name.value.as_str() == "emit"
            {
                // Optionally verify the path includes "event"
                return true;
            }
            false
        }
    }
}

/// Extracts the struct name from an emit() argument.
/// Handles: emit(MyStruct { ... }) and emit(variable) where variable was packed.
fn collect_emitted_struct_name(exp: &Exp, emitted: &mut HashSet<String>) {
    match &exp.value {
        Exp_::Pack(chain, _) => {
            if let Some(name) = extract_name_from_chain(chain) {
                emitted.insert(name);
            }
        }
        Exp_::Name(_chain) => {
            // Variable — could be a struct constructed earlier, but we can't track that
            // without dataflow analysis. Skip for now.
        }
        Exp_::Block(seq) => {
            // The emit arg might be a block that ends with a Pack
            if let Some(trailing) = seq.3.as_ref() {
                collect_emitted_struct_name(trailing, emitted);
            }
        }
        _ => {}
    }
}

/// Extracts a simple name from a NameAccessChain (the last component).
fn extract_name_from_chain(chain: &NameAccessChain) -> Option<String> {
    match &chain.value {
        NameAccessChain_::Single(entry) => Some(entry.name.value.as_str().to_string()),
        NameAccessChain_::Path(path) => path
            .entries
            .last()
            .map(|e| e.name.value.as_str().to_string()),
    }
}

/// Extracts a `ModuleInfo` from a parsed `ModuleDefinition`.
pub fn extract_module(module_def: &ModuleDefinition) -> ModuleInfo {
    let module_name = module_def.name.0.value.as_str().to_string();

    // Extract structs first (needed for singleton detection)
    let structs = extract_structs(module_def);

    // Build constructor map and detect singletons (only key-bearing structs)
    let constructor_map = build_constructor_map(module_def);
    let singletons = detect_singletons(&constructor_map, &structs);

    // Detect emitted events by scanning for event::emit() calls
    let emitted_events = detect_emitted_events(module_def);

    // Extract functions
    let functions = extract_functions(module_def, &singletons);

    ModuleInfo {
        name: module_name,
        functions,
        structs,
        singletons,
        emitted_events,
    }
}

/// Extracts all struct definitions from a module.
fn extract_structs(module_def: &ModuleDefinition) -> Vec<StructInfo> {
    let mut structs = Vec::new();

    for member in &module_def.members {
        if let ModuleMember::Struct(struct_def) = member {
            let name = struct_def.name.0.value.as_str().to_string();
            let has_key = struct_def
                .abilities
                .iter()
                .any(|a| a.value == Ability_::Key);
            let has_copy = struct_def
                .abilities
                .iter()
                .any(|a| a.value == Ability_::Copy);
            let has_drop = struct_def
                .abilities
                .iter()
                .any(|a| a.value == Ability_::Drop);

            // Build type param names for this struct
            let type_param_names: HashSet<String> = struct_def
                .type_parameters
                .iter()
                .map(|tp| tp.name.value.as_str().to_string())
                .collect();

            let fields = match &struct_def.fields {
                StructFields::Named(named_fields) => named_fields
                    .iter()
                    .map(|(_doc, field, ty)| {
                        let field_name = field.0.value.as_str().to_string();
                        let field_type = convert_type(ty, &type_param_names);
                        (field_name, field_type)
                    })
                    .collect(),
                StructFields::Positional(pos_fields) => pos_fields
                    .iter()
                    .enumerate()
                    .map(|(i, (_doc, ty))| {
                        let field_name = format!("field_{i}");
                        let field_type = convert_type(ty, &type_param_names);
                        (field_name, field_type)
                    })
                    .collect(),
                StructFields::Native(_) => vec![],
            };

            structs.push(StructInfo {
                name,
                fields,
                has_key,
                has_copy,
                has_drop,
            });
        }
    }

    structs
}

/// Returns true if a function body is just `abort N` — a deprecated/legacy stub.
fn is_abort_only_body(body: &move_compiler::parser::ast::FunctionBody) -> bool {
    let FunctionBody_::Defined(seq) = &body.value else {
        return false;
    };
    let (_, items, _, trailing) = seq;

    // Case: `{ abort N }` — no items, trailing is Abort
    if items.is_empty() {
        if let Some(exp) = trailing.as_ref() {
            return matches!(&exp.value, Exp_::Abort(_));
        }
    }

    // Case: `{ abort N; }` — single Seq item is Abort
    if items.len() == 1 && trailing.is_none() {
        if let SequenceItem_::Seq(exp) = &items[0].value {
            return matches!(&exp.value, Exp_::Abort(_));
        }
    }

    false
}

/// Extracts all public/entry functions from a module, applying singleton marking.
fn extract_functions(
    module_def: &ModuleDefinition,
    singletons: &HashSet<String>,
) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();

    for member in &module_def.members {
        if let ModuleMember::Function(func) = member {
            let visibility = &func.visibility;
            let is_entry = func.entry.is_some();

            // Skip non-public, non-entry functions (internal, package, friend)
            let is_public = matches!(visibility, Visibility::Public(_));
            if !is_public && !is_entry {
                continue;
            }

            // Skip macro functions
            if func.macro_.is_some() {
                continue;
            }

            // Skip abort/panic-only functions (deprecated/legacy stubs like `abort E_DEPRECATED`)
            if is_abort_only_body(&func.body) {
                continue;
            }

            let func_name = func.name.0.value.as_str().to_string();

            // Extract type parameters
            let type_params: Vec<String> = func
                .signature
                .type_parameters
                .iter()
                .map(|(name, _abilities)| name.value.as_str().to_string())
                .collect();

            // Build type param name set for type conversion
            let type_param_names: HashSet<String> = type_params.iter().cloned().collect();

            // Extract parameters
            let raw_params: Vec<ParamInfo> = func
                .signature
                .parameters
                .iter()
                .map(|(_mutability, var, ty)| {
                    let param_name = var.0.value.as_str().to_string();
                    let move_type = convert_type(ty, &type_param_names);

                    // Check if this parameter's type is a singleton
                    let is_singleton = move_type
                        .struct_name()
                        .map(|sn| singletons.contains(sn))
                        .unwrap_or(false);

                    ParamInfo {
                        name: param_name,
                        move_type,
                        is_singleton,
                    }
                })
                .collect();

            // Process params: strip TxContext, Clock, Random
            let (visible_params, has_clock, has_random) = process_params(raw_params);

            functions.push(FunctionInfo {
                name: func_name,
                is_entry,
                type_params,
                params: visible_params,
                has_clock_param: has_clock,
                has_random_param: has_random,
            });
        }
    }

    functions
}

/// Extracts all modules from a list of parsed `Definition`s.
pub fn extract_modules(defs: &[Definition]) -> Vec<ModuleInfo> {
    let mut modules = Vec::new();

    for def in defs {
        match def {
            Definition::Module(module_def) => {
                modules.push(extract_module(module_def));
            }
            Definition::Address(addr_def) => {
                for module_def in &addr_def.modules {
                    modules.push(extract_module(module_def));
                }
            }
        }
    }

    modules
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::MoveParser;

    fn make_param(name: &str, move_type: MoveType) -> ParamInfo {
        ParamInfo {
            name: name.to_string(),
            move_type,
            is_singleton: false,
        }
    }

    fn make_function(name: &str, is_entry: bool, params: Vec<ParamInfo>) -> FunctionInfo {
        let (visible_params, has_clock, has_random) = process_params(params);
        FunctionInfo {
            name: name.to_string(),
            is_entry,
            type_params: vec![],
            params: visible_params,
            has_clock_param: has_clock,
            has_random_param: has_random,
        }
    }

    // ---- MoveType detection tests ----

    #[test]
    fn tx_context_is_auto_stripped() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "TxContext".to_string(),
                type_args: vec![],
            }),
            is_mut: true,
        };
        assert!(ty.is_tx_context());
        assert!(ty.is_auto_stripped());
        assert!(!ty.is_clock());
        assert!(!ty.is_random());
    }

    #[test]
    fn clock_ref_is_detected() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "Clock".to_string(),
                type_args: vec![],
            }),
            is_mut: false,
        };
        assert!(ty.is_clock());
        assert!(ty.is_auto_stripped());
        assert!(!ty.is_tx_context());
        assert!(!ty.is_random());
    }

    #[test]
    fn random_ref_is_detected() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "Random".to_string(),
                type_args: vec![],
            }),
            is_mut: false,
        };
        assert!(ty.is_random());
        assert!(ty.is_auto_stripped());
        assert!(!ty.is_tx_context());
        assert!(!ty.is_clock());
    }

    #[test]
    fn bare_clock_struct_is_detected() {
        let ty = MoveType::Struct {
            module: None,
            name: "Clock".to_string(),
            type_args: vec![],
        };
        assert!(ty.is_clock());
    }

    #[test]
    fn bare_random_struct_is_detected() {
        let ty = MoveType::Struct {
            module: None,
            name: "Random".to_string(),
            type_args: vec![],
        };
        assert!(ty.is_random());
    }

    #[test]
    fn regular_struct_is_not_auto_stripped() {
        let ty = MoveType::Struct {
            module: None,
            name: "Marketplace".to_string(),
            type_args: vec![],
        };
        assert!(!ty.is_auto_stripped());
        assert!(!ty.is_clock());
        assert!(!ty.is_random());
        assert!(!ty.is_tx_context());
    }

    #[test]
    fn object_ref_detected() {
        let ty = MoveType::Ref {
            inner: Box::new(MoveType::Struct {
                module: None,
                name: "Marketplace".to_string(),
                type_args: vec![],
            }),
            is_mut: true,
        };
        assert!(ty.is_object_ref());
    }

    #[test]
    fn primitive_is_not_object_ref() {
        assert!(!MoveType::U64.is_object_ref());
        assert!(!MoveType::Bool.is_object_ref());
        assert!(!MoveType::Address.is_object_ref());
    }

    // ---- process_params tests ----

    #[test]
    fn strips_tx_context_entirely() {
        let params = vec![
            make_param(
                "marketplace",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Marketplace".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
            make_param("price", MoveType::U64),
            make_param(
                "ctx",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "TxContext".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
        ];

        let (visible, has_clock, has_random) = process_params(params);

        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].name, "marketplace");
        assert_eq!(visible[1].name, "price");
        assert!(!has_clock);
        assert!(!has_random);
    }

    #[test]
    fn strips_clock_and_flags_injection() {
        let params = vec![
            make_param(
                "marketplace",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Marketplace".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: false,
                },
            ),
            make_param(
                "clock",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Clock".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: false,
                },
            ),
            make_param(
                "ctx",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "TxContext".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
        ];

        let (visible, has_clock, has_random) = process_params(params);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "marketplace");
        assert!(has_clock);
        assert!(!has_random);
    }

    #[test]
    fn strips_random_and_flags_injection() {
        let params = vec![
            make_param("amount", MoveType::U64),
            make_param(
                "rng",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Random".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: false,
                },
            ),
            make_param(
                "ctx",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "TxContext".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
        ];

        let (visible, has_clock, has_random) = process_params(params);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "amount");
        assert!(!has_clock);
        assert!(has_random);
    }

    #[test]
    fn strips_both_clock_and_random() {
        let params = vec![
            make_param("value", MoveType::U64),
            make_param(
                "clock",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Clock".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: false,
                },
            ),
            make_param(
                "rng",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Random".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: false,
                },
            ),
            make_param(
                "ctx",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "TxContext".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
        ];

        let (visible, has_clock, has_random) = process_params(params);

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "value");
        assert!(has_clock);
        assert!(has_random);
    }

    #[test]
    fn no_special_params_passes_all_through() {
        let params = vec![
            make_param(
                "pool",
                MoveType::Ref {
                    inner: Box::new(MoveType::Struct {
                        module: None,
                        name: "Pool".to_string(),
                        type_args: vec![],
                    }),
                    is_mut: true,
                },
            ),
            make_param("amount", MoveType::U64),
            make_param("recipient", MoveType::Address),
        ];

        let (visible, has_clock, has_random) = process_params(params);

        assert_eq!(visible.len(), 3);
        assert!(!has_clock);
        assert!(!has_random);
    }

    // ---- filter_functions tests ----

    #[test]
    fn methods_filter_includes_only_specified() {
        let functions = vec![
            make_function("list_item", true, vec![]),
            make_function("cancel_listing", true, vec![]),
            make_function("get_price", false, vec![]),
        ];

        let include = Some(vec!["list_item".to_string(), "get_price".to_string()]);
        let (filtered, warnings) = filter_functions(functions, &include, &None);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "list_item");
        assert_eq!(filtered[1].name, "get_price");
        assert!(warnings.is_empty());
    }

    #[test]
    fn methods_filter_warns_on_missing() {
        let functions = vec![make_function("list_item", true, vec![])];

        let include = Some(vec!["list_item".to_string(), "nonexistent".to_string()]);
        let (filtered, warnings) = filter_functions(functions, &include, &None);

        assert_eq!(filtered.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("nonexistent"));
    }

    #[test]
    fn skip_methods_excludes_specified() {
        let functions = vec![
            make_function("list_item", true, vec![]),
            make_function("cancel_listing", true, vec![]),
            make_function("get_price", false, vec![]),
        ];

        let skip = Some(vec!["cancel_listing".to_string()]);
        let (filtered, warnings) = filter_functions(functions, &None, &skip);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "list_item");
        assert_eq!(filtered[1].name, "get_price");
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_filter_returns_all() {
        let functions = vec![
            make_function("a", false, vec![]),
            make_function("b", true, vec![]),
            make_function("c", false, vec![]),
        ];

        let (filtered, warnings) = filter_functions(functions, &None, &None);

        assert_eq!(filtered.len(), 3);
        assert!(warnings.is_empty());
    }

    // ---- Singleton detection tests ----

    #[test]
    fn singleton_marks_param_correctly() {
        let singletons: HashSet<String> = HashSet::from(["Marketplace".to_string()]);

        let param = ParamInfo {
            name: "marketplace".to_string(),
            move_type: MoveType::Ref {
                inner: Box::new(MoveType::Struct {
                    module: None,
                    name: "Marketplace".to_string(),
                    type_args: vec![],
                }),
                is_mut: true,
            },
            is_singleton: singletons.contains("Marketplace"),
        };

        assert!(param.is_singleton);
    }

    #[test]
    fn non_singleton_param_not_marked() {
        let singletons: HashSet<String> = HashSet::from(["Marketplace".to_string()]);

        let param = ParamInfo {
            name: "listing".to_string(),
            move_type: MoveType::Ref {
                inner: Box::new(MoveType::Struct {
                    module: None,
                    name: "Listing".to_string(),
                    type_args: vec![],
                }),
                is_mut: false,
            },
            is_singleton: singletons.contains("Listing"),
        };

        assert!(!param.is_singleton);
    }

    // ---- MoveType equality tests (for type mapping correctness) ----

    #[test]
    fn vector_u8_special_case() {
        let ty = MoveType::Vector(Box::new(MoveType::U8));
        assert_eq!(ty, MoveType::Vector(Box::new(MoveType::U8)));
        // This pattern must be matched before generic vector<T>
    }

    #[test]
    fn nested_option_vector() {
        let ty = MoveType::Option(Box::new(MoveType::Vector(Box::new(MoveType::U64))));
        match &ty {
            MoveType::Option(inner) => match inner.as_ref() {
                MoveType::Vector(elem) => assert_eq!(**elem, MoveType::U64),
                other => panic!("expected Vector, got {other:?}"),
            },
            other => panic!("expected Option, got {other:?}"),
        }
    }

    #[test]
    fn option_vector_u8_special_case() {
        // Option<vector<u8>> should map to Uint8Array | null
        let ty = MoveType::Option(Box::new(MoveType::Vector(Box::new(MoveType::U8))));
        match &ty {
            MoveType::Option(inner) => {
                assert_eq!(**inner, MoveType::Vector(Box::new(MoveType::U8)));
            }
            other => panic!("expected Option, got {other:?}"),
        }
    }

    #[test]
    fn struct_with_type_args() {
        let coin_sui = MoveType::Struct {
            module: Some("coin".to_string()),
            name: "Coin".to_string(),
            type_args: vec![MoveType::TypeParam("T".to_string())],
        };
        match &coin_sui {
            MoveType::Struct {
                name, type_args, ..
            } => {
                assert_eq!(name, "Coin");
                assert_eq!(type_args.len(), 1);
                assert_eq!(type_args[0], MoveType::TypeParam("T".to_string()));
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    // ---- AST extraction tests ----

    #[test]
    fn extracts_simple_module() {
        let source = r#"
module test_pkg::my_module {
    public fun hello(value: u64): u64 {
        value + 1
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "my_module");
        assert_eq!(modules[0].functions.len(), 1);
        assert_eq!(modules[0].functions[0].name, "hello");
        assert!(!modules[0].functions[0].is_entry);
        assert_eq!(modules[0].functions[0].params.len(), 1);
        assert_eq!(modules[0].functions[0].params[0].name, "value");
        assert_eq!(modules[0].functions[0].params[0].move_type, MoveType::U64);
    }

    #[test]
    fn extracts_entry_function() {
        let source = r#"
module test_pkg::entry_mod {
    public struct MyObj has key {
        id: UID,
        value: u64,
    }

    entry fun do_thing(obj: &mut MyObj, amount: u64, ctx: &mut TxContext) {
        obj.value = amount;
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        assert_eq!(modules.len(), 1);
        let func = &modules[0].functions[0];
        assert_eq!(func.name, "do_thing");
        assert!(func.is_entry);
        // TxContext should be stripped
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "obj");
        assert!(func.params[0].move_type.is_object_ref());
        assert_eq!(func.params[1].name, "amount");
        assert_eq!(func.params[1].move_type, MoveType::U64);
    }

    #[test]
    fn extracts_struct_fields() {
        let source = r#"
module test_pkg::struct_mod {
    public struct Listing has key, store {
        id: UID,
        price: u64,
        seller: address,
    }

    public fun get_price(listing: &Listing): u64 {
        listing.price
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        assert_eq!(modules[0].structs.len(), 1);
        let s = &modules[0].structs[0];
        assert_eq!(s.name, "Listing");
        assert!(s.has_key);
        assert_eq!(s.fields.len(), 3);
        assert_eq!(s.fields[0].0, "id");
        assert_eq!(s.fields[1].0, "price");
        assert_eq!(s.fields[1].1, MoveType::U64);
        assert_eq!(s.fields[2].0, "seller");
        assert_eq!(s.fields[2].1, MoveType::Address);
    }

    #[test]
    fn detects_singleton_from_init() {
        let source = r#"
module test_pkg::singleton_mod {
    public struct Registry has key {
        id: UID,
        count: u64,
    }

    fun init(ctx: &mut TxContext) {
        let registry = Registry {
            id: object::new(ctx),
            count: 0,
        };
        transfer::share_object(registry);
    }

    public fun get_count(registry: &Registry): u64 {
        registry.count
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        assert_eq!(modules.len(), 1);
        assert!(modules[0].singletons.contains("Registry"));
        // The get_count param for Registry should be marked as singleton
        let func = &modules[0].functions[0];
        assert_eq!(func.name, "get_count");
        assert_eq!(func.params.len(), 1);
        assert!(func.params[0].is_singleton);
    }

    #[test]
    fn non_singleton_when_constructed_elsewhere() {
        let source = r#"
module test_pkg::non_singleton_mod {
    public struct Item has key {
        id: UID,
        name: vector<u8>,
    }

    fun init(ctx: &mut TxContext) {
        let item = Item {
            id: object::new(ctx),
            name: b"default",
        };
        transfer::share_object(item);
    }

    public fun create_item(name: vector<u8>, ctx: &mut TxContext): Item {
        Item {
            id: object::new(ctx),
            name,
        }
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        // Item is constructed in both init and create_item, so NOT a singleton
        assert!(!modules[0].singletons.contains("Item"));
    }

    #[test]
    fn extracts_generic_function() {
        let source = r#"
module test_pkg::generic_mod {
    public fun withdraw<T>(pool: &mut Pool<T>, amount: u64, ctx: &mut TxContext) {
        pool.amount = amount;
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let func = &modules[0].functions[0];
        assert_eq!(func.name, "withdraw");
        assert_eq!(func.type_params, vec!["T".to_string()]);
        // TxContext stripped
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].name, "pool");
        assert!(func.params[0].move_type.is_object_ref());
        assert_eq!(func.params[1].name, "amount");
    }

    #[test]
    fn extracts_clock_and_random_params() {
        let source = r#"
module test_pkg::special_mod {
    public fun timed_action(pool: &mut Pool, clock: &Clock, ctx: &mut TxContext) {
        pool.value = 1;
    }

    public fun random_action(pool: &mut Pool, rng: &Random, ctx: &mut TxContext) {
        pool.value = 2;
    }

    public fun both_special(pool: &mut Pool, clock: &Clock, rng: &Random, ctx: &mut TxContext) {
        pool.value = 3;
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let timed = &modules[0].functions[0];
        assert_eq!(timed.name, "timed_action");
        assert!(timed.has_clock_param);
        assert!(!timed.has_random_param);
        assert_eq!(timed.params.len(), 1); // only pool

        let random = &modules[0].functions[1];
        assert_eq!(random.name, "random_action");
        assert!(!random.has_clock_param);
        assert!(random.has_random_param);
        assert_eq!(random.params.len(), 1);

        let both = &modules[0].functions[2];
        assert_eq!(both.name, "both_special");
        assert!(both.has_clock_param);
        assert!(both.has_random_param);
        assert_eq!(both.params.len(), 1);
    }

    #[test]
    fn skips_private_functions() {
        let source = r#"
module test_pkg::visibility_mod {
    public fun pub_fn(a: u64): u64 { a }
    fun private_fn(b: u64): u64 { b }
    entry fun entry_fn(c: u64) { let _x = c; }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let names: Vec<&str> = modules[0]
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(names.contains(&"pub_fn"));
        assert!(names.contains(&"entry_fn"));
        assert!(!names.contains(&"private_fn"));
    }

    #[test]
    fn extracts_vector_and_option_types() {
        let source = r#"
module test_pkg::type_mod {
    public fun process(
        data: vector<u8>,
        amounts: vector<u64>,
        maybe_val: Option<u64>,
    ): bool {
        true
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let func = &modules[0].functions[0];
        assert_eq!(func.params.len(), 3);
        assert_eq!(
            func.params[0].move_type,
            MoveType::Vector(Box::new(MoveType::U8))
        );
        assert_eq!(
            func.params[1].move_type,
            MoveType::Vector(Box::new(MoveType::U64))
        );
        assert_eq!(
            func.params[2].move_type,
            MoveType::Option(Box::new(MoveType::U64))
        );
    }

    #[test]
    fn extracts_bool_and_address_types() {
        let source = r#"
module test_pkg::primitives_mod {
    public fun transfer_to(active: bool, recipient: address) {
        let _a = active;
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let func = &modules[0].functions[0];
        assert_eq!(func.params[0].move_type, MoveType::Bool);
        assert_eq!(func.params[1].move_type, MoveType::Address);
    }

    #[test]
    fn skips_abort_only_functions() {
        // Functions with only `abort N` are deprecated stubs — should be skipped
        let source = r#"
module test_pkg::legacy_mod {
    public fun active_fn(value: u64): u64 { value + 1 }

    public fun deprecated_fn(value: u64) {
        abort 6
    }

    public entry fun deprecated_entry(a: u64, b: u64) {
        abort 0
    }

    public fun also_active(x: bool): bool { x }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        let names: Vec<&str> = modules[0]
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(names, vec!["active_fn", "also_active"]);
        assert!(
            !names.contains(&"deprecated_fn"),
            "abort-only function should be skipped"
        );
        assert!(
            !names.contains(&"deprecated_entry"),
            "abort-only entry should be skipped"
        );
    }

    #[test]
    fn keeps_function_with_abort_plus_other_code() {
        // A function that has abort but also other code should NOT be skipped
        let source = r#"
module test_pkg::mixed_mod {
    public fun guarded_fn(value: u64): u64 {
        assert!(value > 0, 1);
        value
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        let modules = extract_modules(&defs);

        assert_eq!(modules[0].functions.len(), 1);
        assert_eq!(modules[0].functions[0].name, "guarded_fn");
    }
}
