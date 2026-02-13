//! HumanSSH build automation tasks.
//!
//! Usage: `cargo xtask <command>`

use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("new-crate") => {
            if let Some(name) = args.get(1) {
                println!("Would create crate: crates/{name}/");
                println!("  crates/{name}/Cargo.toml");
                println!("  crates/{name}/src/{name}.rs");
            } else {
                eprintln!("Usage: cargo xtask new-crate <name>");
            }
        }
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}");
            print_help();
        }
        None => print_help(),
    }
}

fn print_help() {
    println!("HumanSSH xtask");
    println!();
    println!("USAGE:");
    println!("    cargo xtask <COMMAND>");
    println!();
    println!("COMMANDS:");
    println!("    new-crate <name>    Scaffold a new workspace crate");
}
