//! Compiles every `.cui` file in `ui/` to Rust before the crate itself builds.
//! The result lands in `OUT_DIR` and is pulled in by `src/lib.rs`.

fn main() {
    println!("cargo:rerun-if-changed=ui");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    craftui_dsl::compile_cui_dir("ui", &out_dir).expect("failed compiling notes .cui files");
}
