use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub const LIBRARIES: [&str; 3] = ["slimg_jpegli_shim", "slimg_jpegli", "slimg_jpegli_hwy"];
pub const UPSTREAM: &str = include_str!("upstream.json");

pub fn library_file(name: &str, target: &str) -> String {
    if target.ends_with("-msvc") {
        format!("{name}.lib")
    } else {
        format!("lib{name}.a")
    }
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn manifest(dir: &Path, target: &str, crt: &str, deployment: &str) -> Result<Value, String> {
    let mut files = serde_json::Map::new();
    for name in LIBRARIES {
        let file = format!("lib/{}", library_file(name, target));
        files.insert(file.clone(), json!(sha256(&dir.join(file))?));
    }
    Ok(json!({
        "schema": 1,
        "shim_abi": 2,
        "crate_version": env!("CARGO_PKG_VERSION"),
        "target": target,
        "crt": crt,
        "macos_deployment_target": deployment,
        "upstream": serde_json::from_str::<Value>(UPSTREAM).unwrap(),
        "highway_namespace": "slimg_jpegli_hwy_v1",
        "files": files,
    }))
}

pub fn validate(dir: &Path, target: &str, crt: &str, deployment: &str) -> Result<(), String> {
    let path = dir.join("manifest.json");
    let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let actual: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    let expected = manifest(dir, target, crt, deployment)?;
    for field in [
        "schema",
        "shim_abi",
        "crate_version",
        "target",
        "crt",
        "upstream",
        "highway_namespace",
        "files",
    ] {
        if actual[field] != expected[field] {
            return Err(format!(
                "incompatible or corrupt Jpegli artifact: {field} mismatch"
            ));
        }
    }
    if target.contains("apple-darwin") {
        let minimum = actual["macos_deployment_target"]
            .as_str()
            .ok_or("missing macOS deployment target")?;
        let version = |s: &str| -> Result<Vec<u32>, String> {
            let mut parts = s
                .split('.')
                .map(|v| {
                    v.parse()
                        .map_err(|_| format!("invalid deployment target {s}"))
                })
                .collect::<Result<Vec<u32>, _>>()?;
            parts.resize(3, 0);
            Ok(parts)
        };
        if version(minimum)? > version(deployment)? {
            return Err(format!(
                "Jpegli requires macOS {minimum}, consumer targets {deployment}"
            ));
        }
    }
    Ok(())
}

pub fn artifact_name(target: &str, crt: &str) -> Result<String, String> {
    match target {
        "aarch64-apple-darwin"
        | "x86_64-apple-darwin"
        | "x86_64-unknown-linux-gnu"
        | "aarch64-unknown-linux-gnu"
        | "x86_64-pc-windows-msvc" => Ok(format!("jpegli-prebuilt-{target}-{crt}")),
        _ => Err(format!(
            "no Jpegli prebuilt for {target}; use a vendored source build"
        )),
    }
}
