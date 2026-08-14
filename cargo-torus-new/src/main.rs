//! `cargo-torus-new` — scaffold a new TPT Torus project.
//!
//! Creates a minimal Cargo project that depends on the `torus` ergonomic facade
//! and demonstrates the raw `Flow`/`Operation` API. Equivalent in spirit to
//! `cargo new`, but pre-wired for TPT Torus.
//!
//! Usage:
//! ```text
//! cargo-torus-new <project-name> [--path <dir>]
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

const CARGO_TOML: &str = r#"[package]
name = "{{name}}"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"

[dependencies]
torus = "0.1.0"
"#;

const MAIN_RS: &str = r#"//! A new TPT Torus project.
//!
//! TPT Torus unifies Linux io_uring / Windows IOCP / macOS-BSD kqueue behind a
//! single Virtual Torus ring-buffer API. See https://github.com/tpt-solutions/tpt-torus.

use torus::{open, Flow, Operation};

fn main() -> Result<(), torus::Error> {
    // Open a Torus with the platform-default backend (1024 ring entries).
    let torus = open(1024)?;

    // Submit a read of standard input, then wait for and reap the completion.
    let mut buf = [0u8; 1024];
    let flow = Flow::new(Operation::Read {
        fd: 0,
        buf: buf.as_mut_ptr(),
        len: buf.len(),
        offset: 0,
    });
    torus.submit(&flow)?;
    torus.wait(1_000_000)?;

    let mut results = Vec::new();
    torus.reap(&mut results)?;
    for result in &results {
        if let Some(bytes) = result.bytes() {
            println!("read {} bytes from fd {}", bytes, result.user_data);
        }
    }
    Ok(())
}
"#;

fn main() {
    if let Err(e) = run() {
        eprintln!("cargo-torus-new: {}", e);
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut project_name: Option<String> = None;
    let mut output_dir = PathBuf::from(".");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                if i >= args.len() {
                    return Err("--path requires a directory argument".into());
                }
                output_dir = PathBuf::from(&args[i]);
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                if project_name.is_none() {
                    project_name = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    let name = project_name
        .ok_or("missing project name\nusage: cargo-torus-new <project-name> [--path <dir>]")?;
    if name.is_empty() {
        return Err("project name must not be empty".into());
    }

    let project_dir = output_dir.join(&name);
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| format!("failed to create {}: {}", src_dir.display(), e))?;

    let cargo_toml = CARGO_TOML.replace("{{name}}", &name);
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)
        .map_err(|e| format!("failed to write Cargo.toml: {}", e))?;
    fs::write(src_dir.join("main.rs"), MAIN_RS)
        .map_err(|e| format!("failed to write src/main.rs: {}", e))?;

    println!(
        "Created new TPT Torus project `{}` at {}",
        name,
        project_dir.display()
    );
    Ok(())
}

fn print_help() {
    println!(
        "cargo-torus-new — scaffold a new TPT Torus project\n\n\
         usage: cargo-torus-new <project-name> [--path <dir>]\n\n\
         options:\n  \
         --path <dir>   create the project under <dir> instead of the current directory\n  \
         -h, --help     show this help"
    );
}
