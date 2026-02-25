use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Determine target triple for sidecar naming
    let target_triple = env::var("TARGET").unwrap_or_else(|_| {
        let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
        let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "windows".to_string());
        let env_name =
            env::var("CARGO_CFG_TARGET_ENV").unwrap_or_else(|_| "msvc".to_string());
        format!("{}-pc-{}-{}", arch, os, env_name)
    });

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    // The worker is built by beforeDevCommand / beforeBuildCommand.
    // Its output is in cf-compress-engine/target/{profile}/cf-compress-engine.exe
    let worker_exe = manifest_dir
        .join("cf-compress-engine")
        .join("target")
        .join(&profile)
        .join("cf-compress-engine.exe");

    let binaries_dir = manifest_dir.join("binaries");
    fs::create_dir_all(&binaries_dir).expect("binaries/ ディレクトリの作成に失敗しました");

    let dest = binaries_dir.join(format!("cf-compress-engine-{}.exe", target_triple));

    if worker_exe.exists() {
        fs::copy(&worker_exe, &dest).unwrap_or_else(|e| {
            panic!(
                "ワーカーバイナリのコピーに失敗: {} -> {}: {}",
                worker_exe.display(),
                dest.display(),
                e
            )
        });
    } else {
        panic!(
            "cf-compress-engine バイナリが見つかりません: {}\nbeforeDevCommand / beforeBuildCommand でビルドしてください",
            worker_exe.display()
        );
    }

    println!("cargo:rerun-if-changed=cf-compress-engine/src/");
    println!("cargo:rerun-if-changed=cf-compress-engine/Cargo.toml");

    tauri_build::build();
}
