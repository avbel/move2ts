use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::analyzer::{extract_modules, filter_functions};
use crate::cli::Cli;
use crate::codegen::{
    CodegenConfig, generate_errors_module, generate_module, to_env_var_name, validate_identifier,
};
use crate::parser::MoveParser;

/// Main pipeline: parse Move files, extract IR, generate TypeScript.
pub fn run(cli: &Cli) -> Result<()> {
    // Validate mutually exclusive options
    if cli.methods.is_some() && cli.skip_methods.is_some() {
        bail!("--methods and --skip-methods are mutually exclusive");
    }

    // Determine input type and collect files
    let input = &cli.input;
    let (move_files, project_name) = if input.is_dir() {
        // Package directory mode: read Move.toml for project name
        let move_toml = input.join("Move.toml");
        if !move_toml.exists() {
            bail!(
                "Directory {} does not contain a Move.toml file",
                input.display()
            );
        }

        let toml_content = fs::read_to_string(&move_toml).context("Failed to read Move.toml")?;
        let project = parse_project_name(&toml_content)
            .context("Failed to extract project name from Move.toml")?;

        // Recursively find all .move files in sources/
        let sources_dir = input.join("sources");
        if !sources_dir.exists() {
            bail!(
                "Directory {} does not contain a sources/ directory",
                input.display()
            );
        }

        let files = find_move_files(&sources_dir)?;
        if files.is_empty() {
            bail!("No .move files found in {}", sources_dir.display());
        }

        (files, project)
    } else if input.is_file() {
        // Single file mode: use module name as project name (extracted later)
        let file_name = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("module")
            .to_string();
        (vec![input.clone()], file_name)
    } else {
        bail!("Input path does not exist: {}", input.display());
    };

    // Create output directory
    fs::create_dir_all(&cli.output).context("Failed to create output directory")?;

    // Determine package ID env var name
    let package_id_env_var = cli
        .package_id_name
        .clone()
        .unwrap_or_else(|| format!("{}_PACKAGE_ID", to_env_var_name(&project_name)));

    // Parse all files
    let parser = MoveParser::new();
    let mut all_errors: Vec<String> = Vec::new();
    let mut all_modules = Vec::new();

    for file_path in &move_files {
        let source = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        match parser.parse_source(&source) {
            Ok(defs) => {
                let modules = extract_modules(&defs);
                all_modules.extend(modules);
            }
            Err(err) => {
                all_errors.push(format!("{}: {err}", file_path.display()));
            }
        }
    }

    if !all_errors.is_empty() {
        bail!(
            "Parse errors in {} file(s):\n{}",
            all_errors.len(),
            all_errors.join("\n")
        );
    }

    // Parse singleton overrides from CLI
    let singleton_overrides = parse_singleton_overrides(&cli.singletons)?;

    // Process and generate for each module
    let mut generated_count = 0;

    for module in &mut all_modules {
        // Apply singleton overrides: add any CLI-specified singletons
        for struct_name in &singleton_overrides {
            module.singletons.insert(struct_name.clone());
        }

        // Apply method filter
        let functions = std::mem::take(&mut module.functions);
        let (filtered, warnings) = filter_functions(functions, &cli.methods, &cli.skip_methods);

        for warning in &warnings {
            eprintln!("Warning: {warning}");
        }

        module.functions = filtered;

        // Skip modules with no functions
        if module.functions.is_empty() {
            eprintln!(
                "Warning: module '{}' has no callable functions, skipping",
                module.name
            );
            continue;
        }

        // Validate identifiers to prevent code injection in generated TS
        validate_identifier(&module.name)
            .with_context(|| format!("invalid module name: '{}'", module.name))?;
        for func in &module.functions {
            validate_identifier(&func.name)
                .with_context(|| format!("invalid function name: '{}'", func.name))?;
        }

        let config = CodegenConfig {
            package_id_env_var: package_id_env_var.clone(),
            project_name: project_name.clone(),
            include_events: cli.events,
        };

        let ts_content = generate_module(module, &config);
        let output_file = cli.output.join(format!("{}.ts", module.name));
        fs::write(&output_file, &ts_content)
            .with_context(|| format!("Failed to write {}", output_file.display()))?;

        generated_count += 1;
        println!("Generated: {}", output_file.display());
    }

    if generated_count == 0 {
        println!("\nNo modules generated — all modules were empty.");
    } else {
        // Generate errors module
        let errors_content = generate_errors_module();
        let errors_file = cli.output.join("move2ts-errors.ts");
        fs::write(&errors_file, &errors_content)
            .with_context(|| format!("Failed to write {}", errors_file.display()))?;
        println!("Generated: {}", errors_file.display());

        println!(
            "\nDone: {generated_count} module(s) generated in {}",
            cli.output.display()
        );
    }

    Ok(())
}

/// Parses the project name from Move.toml content.
/// Looks for `name = "..."` in the [package] section.
fn parse_project_name(toml_content: &str) -> Result<String> {
    let mut in_package_section = false;

    for line in toml_content.lines() {
        let trimmed = line.trim();

        // Track sections
        if trimmed.starts_with('[') {
            in_package_section = trimmed == "[package]";
            continue;
        }

        if in_package_section && let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                // Strip quotes
                if let Some(name) = rest.strip_prefix('"')
                    && let Some(name) = name.strip_suffix('"')
                {
                    return Ok(name.to_string());
                }
                if let Some(name) = rest.strip_prefix('\'')
                    && let Some(name) = name.strip_suffix('\'')
                {
                    return Ok(name.to_string());
                }
            }
        }
    }

    bail!("Could not find 'name' in [package] section of Move.toml")
}

/// Recursively finds all .move files in a directory.
fn find_move_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    find_move_files_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn find_move_files_recursive(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            find_move_files_recursive(&path, files)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("move") {
            files.push(path);
        }
    }
    Ok(())
}

/// Parses `--singletons StructName` CLI option into a list of struct names.
/// Env var names are always auto-derived from project + struct name.
fn parse_singleton_overrides(singletons: &Option<Vec<String>>) -> Result<Vec<String>> {
    let mut overrides = Vec::new();

    if let Some(entries) = singletons {
        for entry in entries {
            if entry.is_empty() {
                bail!("Invalid --singletons format: empty entry");
            }
            overrides.push(entry.clone());
        }
    }

    Ok(overrides)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_name_from_toml() {
        let toml = r#"
[package]
name = "my_cool_dex"
version = "0.1.0"

[dependencies]
Sui = { git = "..." }
"#;
        let name = parse_project_name(toml).unwrap();
        assert_eq!(name, "my_cool_dex");
    }

    #[test]
    fn parses_project_name_with_spaces() {
        let toml = r#"
[package]
name  =  "spaced_project"
"#;
        let name = parse_project_name(toml).unwrap();
        assert_eq!(name, "spaced_project");
    }

    #[test]
    fn fails_on_missing_name() {
        let toml = r#"
[package]
version = "0.1.0"
"#;
        let result = parse_project_name(toml);
        assert!(result.is_err());
    }

    #[test]
    fn parses_singleton_overrides_valid() {
        let input = Some(vec!["Marketplace".to_string(), "AdminCap".to_string()]);
        let result = parse_singleton_overrides(&input).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Marketplace");
        assert_eq!(result[1], "AdminCap");
    }

    #[test]
    fn parses_singleton_overrides_none() {
        let result = parse_singleton_overrides(&None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn fails_on_invalid_singleton_format() {
        let input = Some(vec![String::new()]);
        let result = parse_singleton_overrides(&input);
        assert!(result.is_err());
    }
}
