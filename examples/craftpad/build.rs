fn main() {
    println!("cargo:rerun-if-changed=ui");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    craftui_dsl::compile_cui_dir("ui", &out_dir).expect("failed compiling craftpad .cui files");
}
