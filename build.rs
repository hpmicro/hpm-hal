use std::path::PathBuf;
use std::{env, fs};

fn main() {
    // Put the memory definitions somewhere the linker can find it
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rustc-link-search={}", out_dir.display());

    fs::copy("link-xip.x", out_dir.join("link.x")).unwrap();
    println!("cargo:rerun-if-changed=link.x");

    fs::copy("device.x", out_dir.join("device.x")).unwrap();
    println!("cargo:rerun-if-changed=device.x");
}
