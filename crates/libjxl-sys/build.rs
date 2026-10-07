use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const GITHUB_REPO: &str = "yunho-c/slimg";

fn main() {
    println!("cargo:rerun-if-env-changed=LIBJXL_SYS_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-changed=build.rs");
    // docs.rs: no native libs available, emit an empty stub.
    if env::var("DOCS_RS").is_ok() {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        std::fs::write(out_dir.join("bindings.rs"), "// docs.rs stub\n").unwrap();
        return;
    }

    // User-provided prebuilt directory — bypass everything.
    if let Ok(dir) = env::var("LIBJXL_SYS_DIR") {
        let prebuilt = PathBuf::from(&dir);
        link_prebuilt(&prebuilt);
        copy_bindings(&prebuilt.join("bindings.rs"));
        return;
    }

    // Vendored path: build from source if the submodule is checked out.
    #[cfg(feature = "vendored")]
    if Path::new("libjxl/CMakeLists.txt").exists() {
        build_vendored();
        return;
    }

    // Prebuilt download path (crates.io consumers, or vendored without source).
    let prebuilt = download_prebuilt();
    link_prebuilt(&prebuilt);
    copy_bindings(&prebuilt.join("bindings.rs"));
}

// ── Vendored (source) build ─────────────────────────────────────────────────

#[cfg(feature = "vendored")]
fn build_vendored() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let dst = cmake::Config::new("libjxl")
        .profile("Release")
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("JPEGXL_ENABLE_TOOLS", "OFF")
        .define("JPEGXL_ENABLE_DOXYGEN", "OFF")
        .define("JPEGXL_ENABLE_MANPAGES", "OFF")
        .define("JPEGXL_ENABLE_BENCHMARK", "OFF")
        .define("JPEGXL_ENABLE_EXAMPLES", "OFF")
        .define("JPEGXL_ENABLE_SJPEG", "OFF")
        .define("JPEGXL_ENABLE_JPEGLI", "OFF")
        .define("JPEGXL_ENABLE_JPEGLI_LIBJPEG", "OFF")
        .define("JPEGXL_ENABLE_OPENEXR", "OFF")
        .define("JPEGXL_ENABLE_TCMALLOC", "OFF")
        .define("JPEGXL_BUNDLE_LIBPNG", "OFF")
        .define("JPEGXL_ENABLE_SKCMS", "ON")
        .build();

    let lib_dir = dst.join("lib");
    let lib64_dir = dst.join("lib64");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", lib64_dir.display());

    emit_link_libs();

    // bindgen
    let include_dir = dst.join("include");
    let src_include = PathBuf::from("libjxl/lib/include");
    run_bindgen(&src_include, &include_dir, &out_path.join("bindings.rs"));
}

#[cfg(feature = "vendored")]
fn run_bindgen(src_include: &Path, install_include: &Path, out_file: &Path) {
    let target = env::var("TARGET").unwrap();
    bindgen::Builder::default()
        .header(src_include.join("jxl/encode.h").to_str().unwrap())
        .header(src_include.join("jxl/decode.h").to_str().unwrap())
        .header(src_include.join("jxl/types.h").to_str().unwrap())
        .header(
            src_include
                .join("jxl/codestream_header.h")
                .to_str()
                .unwrap(),
        )
        .header(src_include.join("jxl/color_encoding.h").to_str().unwrap())
        .header(
            src_include
                .join("jxl/thread_parallel_runner.h")
                .to_str()
                .unwrap(),
        )
        .clang_arg(format!("-I{}", src_include.display()))
        .clang_arg(format!("-I{}", install_include.display()))
        .clang_arg(format!("--target={target}"))
        // Encoder functions
        .allowlist_function("JxlEncoderCreate")
        .allowlist_function("JxlEncoderDestroy")
        .allowlist_function("JxlEncoderReset")
        .allowlist_function("JxlEncoderSetBasicInfo")
        .allowlist_function("JxlEncoderSetColorEncoding")
        .allowlist_function("JxlEncoderFrameSettingsCreate")
        .allowlist_function("JxlEncoderSetFrameDistance")
        .allowlist_function("JxlEncoderSetFrameLossless")
        .allowlist_function("JxlEncoderFrameSettingsSetOption")
        .allowlist_function("JxlEncoderAddImageFrame")
        .allowlist_function("JxlEncoderCloseInput")
        .allowlist_function("JxlEncoderProcessOutput")
        .allowlist_function("JxlEncoderSetParallelRunner")
        .allowlist_function("JxlEncoderDistanceFromQuality")
        .allowlist_function("JxlColorEncodingSetToSRGB")
        .allowlist_function("JxlThreadParallelRunner")
        .allowlist_function("JxlThreadParallelRunnerCreate")
        .allowlist_function("JxlThreadParallelRunnerDestroy")
        .allowlist_function("JxlThreadParallelRunnerDefaultNumWorkerThreads")
        // Decoder functions
        .allowlist_function("JxlDecoderCreate")
        .allowlist_function("JxlDecoderDestroy")
        .allowlist_function("JxlDecoderReset")
        .allowlist_function("JxlDecoderSubscribeEvents")
        .allowlist_function("JxlDecoderSetInput")
        .allowlist_function("JxlDecoderCloseInput")
        .allowlist_function("JxlDecoderProcessInput")
        .allowlist_function("JxlDecoderGetBasicInfo")
        .allowlist_function("JxlDecoderImageOutBufferSize")
        .allowlist_function("JxlDecoderSetImageOutBuffer")
        .allowlist_function("JxlDecoderReleaseInput")
        .allowlist_function("JxlDecoderSetParallelRunner")
        // Encoder types
        .allowlist_type("JxlEncoderStatus")
        .allowlist_type("JxlEncoderFrameSettingId")
        .allowlist_type("JxlEncoder")
        .allowlist_type("JxlEncoderFrameSettings")
        // Decoder types
        .allowlist_type("JxlDecoder")
        .allowlist_type("JxlDecoderStatus")
        // Shared types
        .allowlist_type("JxlBasicInfo")
        .allowlist_type("JxlPixelFormat")
        .allowlist_type("JxlParallelRunner")
        .allowlist_type("JxlParallelRetCode")
        .allowlist_type("JxlDataType")
        .allowlist_type("JxlEndianness")
        .allowlist_type("JxlColorEncoding")
        .generate()
        .expect("failed to generate libjxl bindings")
        .write_to_file(out_file)
        .expect("failed to write bindings");
}

// ── Prebuilt download ───────────────────────────────────────────────────────

fn download_prebuilt() -> PathBuf {
    let version = env!("CARGO_PKG_VERSION");
    let platform = detect_platform();
    let tag = format!("libjxl-prebuilt-v{version}");
    let archive_name = format!("libjxl-prebuilt-{platform}.tar.gz");
    let url = format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{archive_name}");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let archive_path = out_dir.join(&archive_name);
    let extract_dir = out_dir.join(format!("libjxl-prebuilt-{platform}"));
    let sentinel = out_dir.join(format!(".libjxl-prebuilt-{tag}-{platform}"));

    // Already downloaded and extracted in a previous build run.
    if sentinel.exists() && extract_dir.exists() {
        return extract_dir;
    }

    // Download via curl.
    eprintln!("slimg-libjxl-sys: downloading prebuilt from {url}");
    let status = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "-o",
        ])
        .arg(&archive_path)
        .arg(&url)
        .status()
        .unwrap_or_else(|e| {
            panic!(
                "slimg-libjxl-sys: failed to run curl: {e}\n\
                 Hint: install curl, or set LIBJXL_SYS_DIR, or enable the vendored feature"
            )
        });

    assert!(
        status.success(),
        "slimg-libjxl-sys: failed to download prebuilt (HTTP error).\n\
         URL: {url}\n\
         Hint: check network connectivity, or set LIBJXL_SYS_DIR, or enable the vendored feature"
    );

    // Extract via tar.
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive_path)
        .arg("-C")
        .arg(&out_dir)
        .status()
        .expect("slimg-libjxl-sys: failed to run tar");

    assert!(status.success(), "slimg-libjxl-sys: tar extraction failed");

    // Write sentinel so we skip re-download on incremental builds.
    std::fs::write(&sentinel, &tag).unwrap();

    extract_dir
}

fn detect_platform() -> &'static str {
    let target = env::var("TARGET").unwrap();
    match target.as_str() {
        "x86_64-unknown-linux-gnu" => "linux-x86_64",
        "aarch64-unknown-linux-gnu" => "linux-aarch64",
        "x86_64-apple-darwin" => "macos-x86_64",
        "aarch64-apple-darwin" => "macos-aarch64",
        "x86_64-pc-windows-msvc" => {
            assert!(
                !env::var("CARGO_CFG_TARGET_FEATURE")
                    .unwrap_or_default()
                    .split(',')
                    .any(|f| f == "crt-static"),
                "slimg-libjxl-sys: prebuilts use the dynamic MSVC CRT; use a source build for crt-static"
            );
            "windows-x86_64"
        }
        _ => panic!(
            "slimg-libjxl-sys: no prebuilt for {target}; initialize the vendored source submodule"
        ),
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────────

fn link_prebuilt(prebuilt_dir: &Path) {
    let lib_dir = prebuilt_dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    let windows = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc";
    for name in [
        "jxl",
        "jxl_cms",
        "jxl_threads",
        "hwy",
        "brotlienc",
        "brotlidec",
        "brotlicommon",
    ] {
        let file = if windows {
            format!("{name}.lib")
        } else {
            format!("lib{name}.a")
        };
        let path = lib_dir.join(file);
        assert!(
            path.is_file(),
            "slimg-libjxl-sys: incomplete prebuilt: missing {}",
            path.display()
        );
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        prebuilt_dir.join("bindings.rs").display()
    );
    emit_link_libs();
}

fn emit_link_libs() {
    // libjxl core
    println!("cargo:rustc-link-lib=static=jxl");
    println!("cargo:rustc-link-lib=static=jxl_cms");
    println!("cargo:rustc-link-lib=static=jxl_threads");

    // libjxl vendored dependencies
    println!("cargo:rustc-link-lib=static=hwy");
    println!("cargo:rustc-link-lib=static=brotlienc");
    println!("cargo:rustc-link-lib=static=brotlidec");
    println!("cargo:rustc-link-lib=static=brotlicommon");

    // C++ standard library
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "macos" | "ios" => println!("cargo:rustc-link-lib=c++"),
        "windows" => {} // MSVC links C++ runtime automatically
        _ => println!("cargo:rustc-link-lib=stdc++"),
    }
}

fn copy_bindings(src: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    std::fs::copy(src, out_dir.join("bindings.rs")).unwrap_or_else(|e| {
        panic!(
            "slimg-libjxl-sys: failed to copy bindings from {}: {e}",
            src.display()
        )
    });
}
