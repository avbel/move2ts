use std::collections::{HashMap, HashSet};

use move_compiler::parser::ast::{
    Ability_, Definition, Exp, Exp_, FunctionBody_, ModuleDefinition, ModuleMember,
    NameAccessChain_, Sequence, SequenceItem_, StructFields, Type, Type_, Visibility,
};

/// Represents a Move type in the intermediate representation.
/// Recursive enum — the central IR type for the entire pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum MoveType {
    U8,
    U16,
    U32,
    U64,
    U128,
    U256,
    Bool,
    Address,
    SuiString,
    ObjectId,
    Vector(Box<MoveType>),
    Option(Box<MoveType>),
    Ref {
        inner: Box<MoveType>,
        is_mut: bool,
    },
    TypeParam(String),
    Struct {
        module: std::option::Option<String>,
        name: String,
        type_args: Vec<MoveType>,
    },
    Unit,
}

#[allow(dead_code)]
impl MoveType {
    /// Returns true if this type is a reference to an object (passed via tx.object()).
    pub fn is_object_ref(&self) -> bool {
        matches!(self, MoveType::Ref { .. })
    }

    /// Returns true if this type should be auto-stripped from the generated TS signature.
    /// TxContext is stripped entirely; Clock and Random are stripped but auto-injected.
    pub fn is_auto_stripped(&self) -> bool {
        self.is_tx_context() || self.is_clock() || self.is_random()
    }

    /// Returns true if this is a TxContext parameter (stripped entirely).
    pub fn is_tx_context(&self) -> bool {
        match self {
            MoveType::Ref { inner, .. } => inner.is_tx_context(),
            MoveType::Struct { name, .. } => name == "TxContext",
            _ => false,
        }
    }

    /// Returns true if this is a Clock parameter (auto-injected as tx.object.clock()).
    pub fn is_clock(&self) -> bool {
        match self {
            MoveType::Ref { inner, .. } => inner.is_clock(),
            MoveType::Struct { name, .. } => name == "Clock",
            _ => false,
        }
    }

    /// Returns true if this is a Random parameter (auto-injected as tx.object.random()).
    pub fn is_random(&self) -> bool {
        match self {
            MoveType::Ref { inner, .. } => inner.is_random(),
            MoveType::Struct { name, .. } => name == "Random",
            _ => false,
        }
    }

    /// Returns the struct name if this type (or the inner ref type) is a Struct.
    pub fn struct_name(&self) -> std::option::Option<&str> {
        match self {
            MoveType::Ref { inner, .. } => inner.struct_name(),
            MoveType::Struct { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub functions: Vec<FunctionInfo>,
    pub structs: Vec<StructInfo>,
    pub singletons: HashSet<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FunctionInfo {
    pub name: String,
    pub is_entry: bool,
    pub type_params: Vec<String>,
    pub params: Vec<ParamInfo>,
    pub has_clock_param: bool,
    pub has_random_param: bool,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub move_type: MoveType,
    pub is_singleton: bool,
}

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    pub fields: Vec<(String, MoveType)>,
    pub has_key: bool,
}

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
        Type_::Multiple(types) => {
            // Multiple is used for tuples — treat as Unit for our purposes
            if types.is_empty() {
                MoveType::Unit
            } else {
                // We don't handle tuple returns in the TS wrapper, treat as Unit
                MoveType::Unit
            }
        }
        Type_::Fun(_, _) => MoveType::Unit, // function types not relevant
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

/// Extracts the struct name from a `NameAccessChain` used in a `Pack` expression.
fn extract_pack_struct_name(chain: &move_compiler::parser::ast::NameAccessChain) -> Option<String> {
    match &chain.value {
        NameAccessChain_::Single(entry) => Some(entry.name.value.as_str().to_string()),
        NameAccessChain_::Path(name_path) => name_path
            .entries
            .last()
            .map(|e| e.name.value.as_str().to_string()),
    }
}

/// Recursively walks an expression tree to find all `Pack` (struct constructor) expressions.
/// Collects struct names that are constructed.
fn collect_packs_from_exp(exp: &Exp, constructors: &mut HashSet<String>) {
    match &exp.value {
        Exp_::Pack(chain, fields) => {
            if let Some(name) = extract_pack_struct_name(chain) {
                constructors.insert(name);
            }
            for (_field, field_exp) in fields {
                collect_packs_from_exp(field_exp, constructors);
            }
        }
        Exp_::Call(_, args) => {
            for arg in &args.value {
                collect_packs_from_exp(arg, constructors);
            }
        }
        Exp_::Block(seq) => {
            collect_packs_from_sequence(seq, constructors);
        }
        Exp_::IfElse(cond, then_exp, else_exp) => {
            collect_packs_from_exp(cond, constructors);
            collect_packs_from_exp(then_exp, constructors);
            if let Some(else_e) = else_exp {
                collect_packs_from_exp(else_e, constructors);
            }
        }
        Exp_::While(cond, body) => {
            collect_packs_from_exp(cond, constructors);
            collect_packs_from_exp(body, constructors);
        }
        Exp_::Loop(body) => {
            collect_packs_from_exp(body, constructors);
        }
        Exp_::Labeled(_, inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Assign(lhs, rhs) => {
            collect_packs_from_exp(lhs, constructors);
            collect_packs_from_exp(rhs, constructors);
        }
        Exp_::Return(_, Some(inner)) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Abort(Some(inner)) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Dereference(inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::UnaryExp(_, inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::BinopExp(lhs, _, rhs) => {
            collect_packs_from_exp(lhs, constructors);
            collect_packs_from_exp(rhs, constructors);
        }
        Exp_::Borrow(_, inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Dot(inner, _, _) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::DotCall(inner, _, _, _, _, args) => {
            collect_packs_from_exp(inner, constructors);
            for arg in &args.value {
                collect_packs_from_exp(arg, constructors);
            }
        }
        Exp_::Cast(inner, _) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Annotate(inner, _) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::ExpList(exps) => {
            for e in exps {
                collect_packs_from_exp(e, constructors);
            }
        }
        Exp_::Parens(inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Move(_, inner) | Exp_::Copy(_, inner) => {
            collect_packs_from_exp(inner, constructors);
        }
        Exp_::Lambda(_, _, body) => {
            collect_packs_from_exp(body, constructors);
        }
        Exp_::Vector(_, _, args) => {
            for arg in &args.value {
                collect_packs_from_exp(arg, constructors);
            }
        }
        Exp_::Match(subject, arms) => {
            collect_packs_from_exp(subject, constructors);
            for arm in &arms.value {
                if let Some(guard) = &arm.value.guard {
                    collect_packs_from_exp(guard, constructors);
                }
                collect_packs_from_exp(&arm.value.rhs, constructors);
            }
        }
        Exp_::Index(inner, args) => {
            collect_packs_from_exp(inner, constructors);
            for arg in &args.value {
                collect_packs_from_exp(arg, constructors);
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

/// Recursively walks a sequence (block body) to find Pack expressions.
fn collect_packs_from_sequence(seq: &Sequence, constructors: &mut HashSet<String>) {
    for item in &seq.1 {
        match &item.value {
            SequenceItem_::Seq(exp) => {
                collect_packs_from_exp(exp, constructors);
            }
            SequenceItem_::Bind(_, _, exp) => {
                collect_packs_from_exp(exp, constructors);
            }
            SequenceItem_::Declare(_, _) => {}
        }
    }
    if let Some(trailing_exp) = seq.3.as_ref() {
        collect_packs_from_exp(trailing_exp, constructors);
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
                collect_packs_from_sequence(seq, &mut constructors);

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

/// Detects singletons: structs only constructed in `init()`.
fn detect_singletons(constructor_map: &HashMap<String, HashSet<String>>) -> HashSet<String> {
    let mut singletons = HashSet::new();

    for (struct_name, constructing_fns) in constructor_map {
        if constructing_fns.len() == 1 && constructing_fns.contains("init") {
            singletons.insert(struct_name.clone());
        }
    }

    singletons
}

/// Extracts a `ModuleInfo` from a parsed `ModuleDefinition`.
pub fn extract_module(module_def: &ModuleDefinition) -> ModuleInfo {
    let module_name = module_def.name.0.value.as_str().to_string();

    // Build constructor map and detect singletons
    let constructor_map = build_constructor_map(module_def);
    let singletons = detect_singletons(&constructor_map);

    // Extract structs
    let structs = extract_structs(module_def);

    // Extract functions
    let functions = extract_functions(module_def, &singletons);

    ModuleInfo {
        name: module_name,
        functions,
        structs,
        singletons,
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
            });
        }
    }

    structs
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
        abort 0
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
        abort 0
    }

    public fun random_action(pool: &mut Pool, rng: &Random, ctx: &mut TxContext) {
        abort 0
    }

    public fun both_special(pool: &mut Pool, clock: &Clock, rng: &Random, ctx: &mut TxContext) {
        abort 0
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
    entry fun entry_fn(c: u64) { abort 0 }
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
        abort 0
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
}
