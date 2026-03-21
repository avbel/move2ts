use std::process;

use clap::Parser;
use move2ts::cli::Cli;
use move2ts::driver;

fn main() {
    let cli = Cli::parse();

    if let Err(err) = driver::run(&cli) {
        eprintln!("Error: {err:#}");
        process::exit(1);
    }
}
