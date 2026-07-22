//! Generate C header from Rust FFI declarations using cbindgen.
//!
//! Run with: `cargo run --bin generate-header -p tpt-torus-cxx`
//!
//! This generates `include/torus_generated.h` which can be included by
//! `torus.hpp` or used directly from C code.

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let config_path = format!("{}/cbindgen.toml", crate_dir);
    let output_path = format!("{}/include/torus_generated.h", crate_dir);

    let config = cbindgen::Config::from_file(&config_path).expect("Failed to read cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(&output_path);

    println!("Generated header: {}", output_path);
}
