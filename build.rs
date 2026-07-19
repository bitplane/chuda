use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};
fn main() {
    if env::var_os("CARGO_FEATURE_CUDA").is_none() {
        return;
    }
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
    println!("cargo:rerun-if-env-changed=CHUDA_STATIC_CUDART");
    let library_paths = [
        Path::new("/usr/local/cuda/lib64"),
        Path::new("/usr/lib/x86_64-linux-gnu"),
        Path::new("/usr/lib/aarch64-linux-gnu"),
    ];
    for path in library_paths {
        if path.is_dir() {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }
    println!("cargo:rustc-link-lib=static=renderer_cuda");
    let has_static_runtime = library_paths
        .iter()
        .any(|path| path.join("libcudart_static.a").is_file());
    if has_static_runtime {
        println!("cargo:rustc-link-lib=static=cudart_static");
    } else {
        assert!(
            env::var_os("CHUDA_STATIC_CUDART").is_none(),
            "CHUDA_STATIC_CUDART requires libcudart_static.a"
        );
        println!(
            "cargo:warning=libcudart_static.a not found; using dynamic libcudart for this build"
        );
        println!("cargo:rustc-link-lib=dylib=cudart");
    }
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-lib=dylib=dl");
    println!("cargo:rustc-link-lib=dylib=rt");
    println!("cargo:rustc-link-lib=dylib=pthread");
}
fn check(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("failed to run {command:?}: {e}"));
    assert!(status.success(), "command failed: {command:?}");
}
