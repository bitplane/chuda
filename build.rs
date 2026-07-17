use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=cuda/renderer.cu");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let obj = out.join("renderer.o");
    let lib = out.join("librenderer_cuda.a");
    check(
        Command::new("nvcc")
            .args([
                "-O3",
                "--use_fast_math",
                "-std=c++17",
                "-Xcompiler",
                "-fPIC",
                "-c",
                "cuda/renderer.cu",
                "-o",
            ])
            .arg(&obj),
    );
    check(Command::new("ar").arg("crus").arg(&lib).arg(&obj));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=static=renderer_cuda");
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
fn check(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("failed to run {command:?}: {e}"));
    assert!(status.success(), "command failed: {command:?}");
}
