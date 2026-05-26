use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let def_path = manifest_dir.join("biturbo.def");

    println!("cargo:rerun-if-changed={}", def_path.display());
    // Only apply the .def export map when linking the cdylib.
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path.display());
}

