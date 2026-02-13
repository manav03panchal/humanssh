//! HumanSSH build automation tasks.
//!
//! Usage: `cargo xtask <command>`

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
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
            ExitCode::SUCCESS
        }
        Some("package-conformity") => package_conformity(),
        Some(cmd) => {
            eprintln!("Unknown command: {cmd}");
            print_help();
            ExitCode::FAILURE
        }
        None => {
            print_help();
            ExitCode::SUCCESS
        }
    }
}

fn print_help() {
    println!("HumanSSH xtask");
    println!();
    println!("USAGE:");
    println!("    cargo xtask <COMMAND>");
    println!();
    println!("COMMANDS:");
    println!("    new-crate <name>       Scaffold a new workspace crate");
    println!("    package-conformity     Check workspace crates for policy conformity");
}

/// Check that all workspace member Cargo.toml files conform to project policies:
/// 1. Each must have `[lints] workspace = true`
/// 2. Dependencies must use `workspace = true` (not inline versions)
fn package_conformity() -> ExitCode {
    let workspace_root = workspace_root();
    let workspace_cargo = workspace_root.join("Cargo.toml");

    let contents = fs::read_to_string(&workspace_cargo).unwrap_or_else(|e| {
        panic!("Failed to read {}: {e}", workspace_cargo.display());
    });

    let root: toml::Value = contents.parse().unwrap_or_else(|e| {
        panic!("Failed to parse {}: {e}", workspace_cargo.display());
    });

    let members = root
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .expect("workspace.members not found in root Cargo.toml");

    let mut violations: Vec<String> = Vec::new();

    for member in members {
        let member_path = member.as_str().unwrap();
        let cargo_path = workspace_root.join(member_path).join("Cargo.toml");

        if !cargo_path.exists() {
            violations.push(format!("{member_path}: Cargo.toml not found"));
            continue;
        }

        let contents = fs::read_to_string(&cargo_path).unwrap_or_else(|e| {
            panic!("Failed to read {}: {e}", cargo_path.display());
        });

        let doc: toml::Value = contents.parse().unwrap_or_else(|e| {
            panic!("Failed to parse {}: {e}", cargo_path.display());
        });

        // Check [lints] workspace = true
        let has_workspace_lints = doc
            .get("lints")
            .and_then(|l| l.get("workspace"))
            .and_then(|w| w.as_bool())
            .unwrap_or(false);

        if !has_workspace_lints {
            violations.push(format!("{member_path}: missing `[lints] workspace = true`"));
        }

        // Check dependencies and dev-dependencies use workspace = true
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = doc.get(section).and_then(|d| d.as_table()) {
                for (dep_name, dep_value) in deps {
                    let uses_workspace = dep_value
                        .get("workspace")
                        .and_then(|w| w.as_bool())
                        .unwrap_or(false);

                    if !uses_workspace {
                        violations.push(format!(
                            "{member_path}: {section}.{dep_name} does not use `workspace = true`"
                        ));
                    }
                }
            }
        }
    }

    if violations.is_empty() {
        println!("All workspace members conform to project policies.");
        ExitCode::SUCCESS
    } else {
        eprintln!("Package conformity violations found:\n");
        for v in &violations {
            eprintln!("  - {v}");
        }
        eprintln!("\n{} violation(s) found.", violations.len());
        ExitCode::FAILURE
    }
}

fn workspace_root() -> PathBuf {
    // xtask lives at tooling/xtask, so workspace root is two levels up
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
        // Fallback: walk up from current exe
        let exe = env::current_exe().expect("cannot determine exe path");
        exe.parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .and_then(Path::parent)
            .expect("cannot determine workspace root from exe path")
            .to_string_lossy()
            .into_owned()
    });
    Path::new(&manifest_dir)
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && is_workspace_root(&p.join("Cargo.toml")))
        .expect("cannot find workspace root")
        .to_path_buf()
}

fn is_workspace_root(cargo_toml: &Path) -> bool {
    fs::read_to_string(cargo_toml)
        .map(|c| c.contains("[workspace]"))
        .unwrap_or(false)
}
