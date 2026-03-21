mod analyzer;
mod cli;
mod codegen;
mod driver;
mod parser;

use std::process;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    if let Err(err) = driver::run(&cli) {
        eprintln!("Error: {err:#}");
        process::exit(1);
    }
}
