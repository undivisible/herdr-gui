fn main() {
    let lib_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/ghostty-vt/lib");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("darwin") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/ghostty-vt/lib/libghostty-vt.a");
}
