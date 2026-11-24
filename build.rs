use std::{env, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor/sherpa-onnx");
    let lib = root.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
    println!("cargo:rustc-link-lib=dylib=onnxruntime");
    // dev runs find the dylibs in vendor/; the .app bundle copies them to Contents/Frameworks.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    let header = root.join("include/sherpa-onnx/c-api/c-api.h");
    println!("cargo:rerun-if-changed={}", header.display());
    bindgen::Builder::default()
        .header(header.to_str().unwrap())
        .allowlist_function("SherpaOnnx.*")
        .allowlist_type("SherpaOnnx.*")
        .derive_default(true)
        .generate()
        .expect("bindgen sherpa-onnx")
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("sherpa.rs"))
        .unwrap();
}
