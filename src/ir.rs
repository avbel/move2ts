use std::collections::HashSet;

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
    VecMap(Box<MoveType>, Box<MoveType>),
    Ref {
        inner: Box<MoveType>,
        is_mut: bool,
    },
    TypeParam {
        name: String,
        has_key: bool,
    },
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
    pub emitted_events: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParamInfo {
    pub name: String,
    pub has_key: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FunctionInfo {
    pub name: String,
    pub is_entry: bool,
    pub type_params: Vec<TypeParamInfo>,
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
    pub has_copy: bool,
    pub has_drop: bool,
}

impl StructInfo {
    /// A pure value struct has copy+drop but no key — passed via BCS serialization, not tx.object().
    pub fn is_pure_value(&self) -> bool {
        self.has_copy && self.has_drop && !self.has_key
    }
}
