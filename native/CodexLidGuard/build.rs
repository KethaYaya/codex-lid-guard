fn main() {
    let manifest = std::path::Path::new("app.manifest")
        .canonicalize()
        .expect("app.manifest must exist");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
