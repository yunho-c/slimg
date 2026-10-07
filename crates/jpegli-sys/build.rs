mod build_support;

use build_support::*;
#[cfg(feature = "vendored")]
use std::path::Path;
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    for key in [
        "JPEGLI_SYS_DIR",
        "JPEGLI_SYS_SOURCE_DIR",
        "DOCS_RS",
        "MACOSX_DEPLOYMENT_TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    for path in [
        "build.rs",
        "build_support.rs",
        "upstream.json",
        "native",
        "shim",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    if env::var_os("DOCS_RS").is_some() {
        return;
    }
    let target = env::var("TARGET").unwrap();
    let crt = if target.ends_with("-msvc") {
        if env::var("CARGO_CFG_TARGET_FEATURE")
            .unwrap_or_default()
            .split(',')
            .any(|f| f == "crt-static")
        {
            "mt"
        } else {
            "md"
        }
    } else {
        "native"
    };
    // Rust's platform baselines; CI publishes at these same minimum versions.
    let deployment = if target.contains("apple-darwin") {
        env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| {
            if target.starts_with("aarch64") {
                "11.0"
            } else {
                "10.13"
            }
            .into()
        })
    } else {
        String::new()
    };
    let dir = resolve(&target, crt, &deployment);
    validate(&dir, &target, crt, &deployment).unwrap_or_else(|e| panic!("slimg-jpegli-sys: {e}"));
    println!(
        "cargo:rerun-if-changed={}",
        dir.join("manifest.json").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        dir.join("lib").display()
    );
    for name in LIBRARIES {
        println!(
            "cargo:rerun-if-changed={}",
            dir.join("lib").join(library_file(name, &target)).display()
        );
        println!("cargo:rustc-link-lib=static={name}");
    }
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else if !target.ends_with("-msvc") {
        println!("cargo:rustc-link-lib=stdc++");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=m");
    }
    println!("cargo:root={}", dir.display());
}

fn resolve(target: &str, crt: &str, deployment: &str) -> PathBuf {
    if let Some(dir) = env::var_os("JPEGLI_SYS_DIR") {
        return PathBuf::from(dir);
    }
    let source = env::var_os("JPEGLI_SYS_SOURCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("jpegli"));
    #[cfg(feature = "vendored")]
    if source.join("CMakeLists.txt").is_file() {
        return build_source(&source, target, crt, deployment);
    }
    if env::var_os("JPEGLI_SYS_SOURCE_DIR").is_some() {
        panic!(
            "slimg-jpegli-sys: JPEGLI_SYS_SOURCE_DIR requires the vendored feature and a populated pinned checkout"
        );
    }
    let _ = (source, deployment);
    download(target, crt)
}

#[cfg(feature = "vendored")]
fn build_source(source: &Path, target: &str, crt: &str, deployment: &str) -> PathBuf {
    let source = source.canonicalize().unwrap();
    let pins: serde_json::Value = serde_json::from_str(UPSTREAM).unwrap();
    for (path, revision) in pins.as_object().unwrap() {
        let dir = if path == "jpegli" {
            source.clone()
        } else {
            source.join(path)
        };
        let output = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("git is required for pinned source builds");
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == revision.as_str().unwrap(),
            "slimg-jpegli-sys: {path} must be initialized at {revision}; see README.md for submodule setup"
        );
    }
    println!("cargo:rerun-if-changed={}", source.display());
    let mut config = cmake::Config::new("native");
    config
        .profile("Release")
        .out_dir(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("native"))
        .define("SLIMG_JPEGLI_SOURCE", &source);
    if target.ends_with("-msvc") {
        config.static_crt(crt == "mt").define(
            "CMAKE_MSVC_RUNTIME_LIBRARY",
            if crt == "mt" {
                "MultiThreaded"
            } else {
                "MultiThreadedDLL"
            },
        );
    }
    if target.contains("apple-darwin") {
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", deployment);
    }
    let dir = config.build();
    let mut metadata = manifest(&dir, target, crt, deployment).unwrap();
    // Keep the compiler, flags, sysroot and CMake configuration with each artifact.
    fs::copy(
        dir.join("build/CMakeCache.txt"),
        dir.join("build-config.txt"),
    )
    .unwrap();
    metadata["build_config_sha256"] =
        serde_json::json!(sha256(&dir.join("build-config.txt")).unwrap());
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
    dir
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("slimg-jpegli-sys: failed to run {command:?}: {e}"));
    assert!(
        status.success(),
        "slimg-jpegli-sys: command failed: {command:?}; set JPEGLI_SYS_DIR for offline builds or initialize the source submodule"
    );
}

fn download(target: &str, crt: &str) -> PathBuf {
    let name = artifact_name(target, crt).unwrap_or_else(|e| panic!("slimg-jpegli-sys: {e}"));
    let tag = format!("jpegli-prebuilt-v{}", env!("CARGO_PKG_VERSION"));
    let base = format!("https://github.com/yunho-c/slimg/releases/download/{tag}/{name}.tar.gz");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let dir = out.join(&name);
    if dir.join("manifest.json").exists() {
        return dir;
    }
    let archive = out.join(format!("{name}.tar.gz"));
    let checksum = out.join(format!("{name}.tar.gz.sha256"));
    for (url, path) in [(&base, &archive), (&format!("{base}.sha256"), &checksum)] {
        run(Command::new("curl")
            .args([
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--tlsv1.2",
                "-fLsS",
                "--retry",
                "3",
                "-o",
            ])
            .arg(path)
            .arg(url));
    }
    let expected = fs::read_to_string(checksum).expect("read Jpegli checksum");
    assert_eq!(
        Some(sha256(&archive).unwrap().as_str()),
        expected.split_whitespace().next(),
        "slimg-jpegli-sys: archive checksum mismatch"
    );
    // Extract only after checking the digest, into an isolated staging directory.
    let staging = out.join("jpegli-extract");
    if staging.exists() {
        fs::remove_dir_all(&staging).unwrap();
    }
    fs::create_dir_all(&staging).unwrap();
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging));
    fs::rename(staging.join(&name), &dir).expect("Jpegli archive has an invalid root directory");
    dir
}
