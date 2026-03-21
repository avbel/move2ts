use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use move_command_line_common::files::FileHash;
use move_compiler::{
    editions::Edition,
    parser::{ast::Definition, syntax::parse_file_string},
    shared::{CompilationEnv, Flags, PackageConfig},
};
use move_symbol_pool::Symbol;

/// Wraps the move-compiler parser. Stateless — creates a fresh CompilationEnv
/// for each `parse_source` call to prevent diagnostic leakage between files.
pub struct MoveParser;

impl Default for MoveParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MoveParser {
    pub fn new() -> Self {
        Self
    }

    /// Creates a fresh `CompilationEnv` for a single parse invocation.
    fn make_env() -> CompilationEnv {
        CompilationEnv::new(
            Flags::empty(),
            vec![],
            vec![],
            None,
            BTreeMap::new(),
            Some(PackageConfig {
                edition: Edition::E2024_BETA,
                ..Default::default()
            }),
            None,
        )
    }

    /// Parse a Move source string into a list of AST definitions.
    pub fn parse_source(&self, source: &str) -> Result<Vec<Definition>> {
        let env = Self::make_env();
        let file_hash = FileHash::new(source);
        parse_file_string(&env, file_hash, source, Some(Symbol::from("move2ts"))).map_err(|diags| {
            let messages: Vec<String> = diags
                .into_codespan_format()
                .into_iter()
                .map(|(severity, msg, _primary, _secondary, _notes)| format!("{severity:?}: {msg}"))
                .collect();
            anyhow!("Parse errors:\n{}", messages.join("\n"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use move_compiler::parser::ast::Definition;

    #[test]
    fn parses_simple_module() {
        let source = r#"
module test_pkg::my_module {
    public fun hello(value: u64): u64 {
        value + 1
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");

        assert!(!defs.is_empty());
        let has_module = defs.iter().any(|d| matches!(d, Definition::Module(_)));
        assert!(has_module, "expected a module definition");
    }

    #[test]
    fn parses_entry_function() {
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
        assert!(!defs.is_empty());
    }

    #[test]
    fn parses_function_with_generics() {
        let source = r#"
module test_pkg::generic_mod {
    public fun withdraw<T>(pool: &mut Pool<T>, amount: u64, ctx: &mut TxContext): Coin<T> {
        abort 0
    }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        assert!(!defs.is_empty());
    }

    #[test]
    fn parses_init_with_share_object() {
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
        assert!(!defs.is_empty());
    }

    #[test]
    fn parses_clock_and_random_params() {
        let source = r#"
module test_pkg::special_params {
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
        assert!(!defs.is_empty());
    }

    // Note: The Move parser is error-recovering — it always returns Ok with partial results.
    // Parse errors are accumulated internally. We rely on the analyzer to detect incomplete AST.

    #[test]
    fn parses_multiple_functions() {
        let source = r#"
module test_pkg::multi {
    public fun foo(a: u64): u64 { a }
    public fun bar(b: bool): bool { b }
    entry fun baz(c: address) { abort 0 }
}
"#;
        let parser = MoveParser::new();
        let defs = parser.parse_source(source).expect("should parse");
        assert!(!defs.is_empty());
    }

    #[test]
    fn parser_reuse_across_files() {
        let parser = MoveParser::new();

        let source1 = r#"
module pkg::mod1 {
    public fun f1(): u64 { 0 }
}
"#;
        let source2 = r#"
module pkg::mod2 {
    public fun f2(): bool { true }
}
"#;

        let defs1 = parser.parse_source(source1).expect("should parse first");
        let defs2 = parser.parse_source(source2).expect("should parse second");

        assert!(!defs1.is_empty());
        assert!(!defs2.is_empty());
    }
}
