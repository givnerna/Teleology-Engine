use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = PathBuf::from(&manifest).join("..").join("..").join("cpp").join("include");
    std::fs::create_dir_all(&out).ok();
    let header = out.join("teleology_ffi.h"); // types only; see cpp/include/teleology.h for full API

    cbindgen::Builder::new()
        .with_crate(manifest)
        .with_language(cbindgen::Language::C)
        .with_include_guard("TELEOLOGY_SCRIPT_API_H")
        .generate()
        .expect("cbindgen failed")
        .write_to_file(header);
}
