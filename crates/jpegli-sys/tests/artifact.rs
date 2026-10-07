#[allow(dead_code)]
#[path = "../build_support.rs"]
mod support;

use std::fs;
use support::*;

#[test]
fn rejects_incompatible_and_corrupt_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    fs::create_dir(dir.join("lib")).unwrap();
    let target = "aarch64-apple-darwin";
    for lib in LIBRARIES {
        fs::write(
            dir.join("lib").join(library_file(lib, target)),
            b"fixture archive",
        )
        .unwrap();
    }
    let original = manifest(dir, target, "native", "11.0").unwrap();
    let save = |value: &serde_json::Value| {
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap()
    };
    save(&original);
    validate(dir, target, "native", "11.0").unwrap();
    validate(dir, target, "native", "12.0").unwrap();
    assert!(
        validate(dir, target, "native", "10.15")
            .unwrap_err()
            .contains("requires macOS")
    );
    for key in [
        "schema",
        "shim_abi",
        "crate_version",
        "target",
        "crt",
        "upstream",
        "highway_namespace",
    ] {
        let mut bad = original.clone();
        bad[key] = serde_json::json!("wrong");
        save(&bad);
        assert!(
            validate(dir, target, "native", "11.0")
                .unwrap_err()
                .contains(key)
        );
    }
    save(&original);
    let library = dir.join("lib").join(library_file(LIBRARIES[0], target));
    fs::write(&library, b"corrupt").unwrap();
    assert!(
        validate(dir, target, "native", "11.0")
            .unwrap_err()
            .contains("files mismatch")
    );
    fs::remove_file(library).unwrap();
    assert!(validate(dir, target, "native", "11.0").is_err());
}

#[test]
fn artifact_identity_includes_target_and_crt() {
    assert_ne!(
        artifact_name("x86_64-pc-windows-msvc", "md").unwrap(),
        artifact_name("x86_64-pc-windows-msvc", "mt").unwrap()
    );
    assert!(artifact_name("x86_64-unknown-linux-musl", "native").is_err());
    assert!(artifact_name("x86_64-pc-windows-gnu", "native").is_err());
}
