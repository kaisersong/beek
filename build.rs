use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn sources(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}
fn main() {
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("build.rs"),
        PathBuf::from(".cargo/config.toml"),
        PathBuf::from("rust-toolchain.toml"),
        PathBuf::from("src/main.rs"),
        PathBuf::from("src/lib.rs"),
        PathBuf::from("src/icons.rs"),
        PathBuf::from("src/bin/selection-host.rs"),
        PathBuf::from("resources/windows/beek.rc"),
        PathBuf::from("resources/windows/BeeK.ico"),
    ];
    sources(Path::new("src/core"), &mut files);
    sources(Path::new("src/platform/windows"), &mut files);
    for entry in fs::read_dir("resources/icons").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|s| s == "png") {
            files.push(path);
        }
    }
    files.sort();
    let mut hash = Sha256::new();
    for key in [
        "CARGO_CFG_TARGET_OS",
        "CARGO_CFG_TARGET_ARCH",
        "CARGO_FEATURE_TEST_FIXTURES",
        "PROFILE",
        "OPT_LEVEL",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        hash.update(std::env::var(key).unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("cannot inspect compiler");
    assert!(compiler.status.success());
    let compiler = String::from_utf8(compiler.stdout).unwrap();
    hash.update(compiler.trim().as_bytes());
    println!("cargo:rustc-env=BEEK_RUSTC_VERSION={}", compiler.trim());
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        hash.update(path.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
        hash.update(fs::read(path).unwrap());
    }
    let build_id = format!("{:x}", hash.finalize());
    println!("cargo:rustc-env=BEEK_BUILD_ID={build_id}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/windows/beek.rc");
        println!("cargo:rerun-if-changed=resources/windows/BeeK.ico");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let numbers = format!("{},0", version.replace('.', ","));
        let rc = fs::read_to_string("resources/windows/beek.rc")
            .unwrap()
            .replace("@VERSION@", &version)
            .replace("@VERSION_NUMBERS@", &numbers);
        let rc = rc.replace("@BUILD_ID@", &build_id);
        let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("beek.rc");
        fs::write(&out, rc).unwrap();
        embed_resource::compile_for(out, ["beek", "beek-selection-host"], embed_resource::NONE)
            .manifest_required()
            .expect("Windows version/icon resource compilation failed");
    }
}
