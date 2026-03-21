use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "move2ts",
    about = "Generate TypeScript wrappers from Sui Move source files"
)]
pub struct Cli {
    /// .move file or package directory (with Move.toml)
    pub input: PathBuf,

    /// Output directory
    #[arg(short, long, default_value = "./generated")]
    pub output: PathBuf,

    /// Generate only these methods (comma-separated, snake_case)
    #[arg(long, value_delimiter = ',')]
    pub methods: Option<Vec<String>>,

    /// Skip these methods (comma-separated, snake_case)
    #[arg(long, value_delimiter = ',')]
    pub skip_methods: Option<Vec<String>>,

    /// Manual singleton overrides (struct names, comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub singletons: Option<Vec<String>>,

    /// Override package ID env var name
    #[arg(long)]
    pub package_id_name: Option<String>,
}
