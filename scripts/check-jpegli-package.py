#!/usr/bin/env python3
"""Verify the distributable crate against a prebuilt, without the source tree."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--prebuilt", type=Path, required=True)
parser.add_argument("--archive", type=Path, help="also exercise the download route using a local curl fixture")
args = parser.parse_args()
prebuilt = args.prebuilt.resolve()
metadata = json.loads((prebuilt / "manifest.json").read_text())
target = metadata["target"]
env = dict(os.environ, JPEGLI_SYS_DIR=str(prebuilt))
if metadata["crt"] == "mt":
    env["RUSTFLAGS"] = "-C target-feature=+crt-static"
if metadata["macos_deployment_target"]:
    env["MACOSX_DEPLOYMENT_TARGET"] = metadata["macos_deployment_target"]
with tempfile.TemporaryDirectory(prefix="slimg-jpegli-package-") as temporary:
    temporary = Path(temporary)
    # An explicit target directory avoids any existing source build outputs.
    env["CARGO_TARGET_DIR"] = str(temporary / "target")
    subprocess.run(["cargo", "package", "-p", "slimg-jpegli-sys", "--allow-dirty", "--no-verify"], env=env, check=True)
    package = temporary / "target/package" / f"slimg-jpegli-sys-{metadata['crate_version']}.crate"
    with tarfile.open(package) as archive:
        archive.extractall(temporary, filter="data")
    crate = temporary / f"slimg-jpegli-sys-{metadata['crate_version']}"
    assert not (crate / "jpegli").exists(), "source tree unexpectedly included"
    assert (crate / "shim/jpegli_shim.cc").is_file(), "missing shim source"
    subprocess.run(["cargo", "run", "--manifest-path", str(crate / "Cargo.toml"), "--no-default-features", "--example", "smoke", "--target", target], env=env, check=True)
    subprocess.run(["cargo", "test", "--manifest-path", str(crate / "Cargo.toml"), "--no-default-features", "--target", target], env=env, check=True)
    if args.archive:
        # No release is needed to test URL selection, extraction, checksum
        # rejection and linking. Only the HTTP transport is replaced here.
        transport = temporary / "transport"
        transport.mkdir()
        mock = transport / "curl.rs"
        mock.write_text('''
use std::{env, fs, path::PathBuf};
fn main() {
    let args: Vec<String> = env::args().collect();
    assert!(args.windows(2).any(|a| a == ["--proto", "=https"]));
    let output = &args[args.iter().position(|a| a == "-o").unwrap() + 1];
    let url = args.last().unwrap();
    let base = env::var("SLIMG_FIXTURE_URL").unwrap();
    assert!(url == &base || url == &(base.clone() + ".sha256"));
    let mut input = PathBuf::from(env::var_os("SLIMG_FIXTURE_ARCHIVE").unwrap());
    if url.ends_with(".sha256") {
        if env::var_os("SLIMG_FIXTURE_CORRUPT").is_some() {
            fs::write(output, "0000000000000000000000000000000000000000000000000000000000000000").unwrap();
            return;
        }
        input = PathBuf::from(format!("{}.sha256", input.display()));
    }
    fs::copy(input, output).unwrap();
}
''')
        executable = transport / ("curl.exe" if os.name == "nt" else "curl")
        subprocess.run(["rustc", str(mock), "-o", str(executable)], check=True)
        downloaded = dict(env)
        downloaded.pop("JPEGLI_SYS_DIR")
        downloaded["PATH"] = str(transport) + os.pathsep + env["PATH"]
        downloaded["SLIMG_FIXTURE_ARCHIVE"] = str(args.archive.resolve())
        downloaded["SLIMG_FIXTURE_URL"] = f"https://github.com/yunho-c/slimg/releases/download/jpegli-prebuilt-v{metadata['crate_version']}/{args.archive.name}"
        downloaded["CARGO_TARGET_DIR"] = str(temporary / "download-target")
        command = ["cargo", "run", "--manifest-path", str(crate / "Cargo.toml"), "--no-default-features", "--example", "smoke", "--target", target]
        subprocess.run(command, env=downloaded, check=True)
        downloaded["CARGO_TARGET_DIR"] = str(temporary / "corrupt-target")
        downloaded["SLIMG_FIXTURE_CORRUPT"] = "1"
        rejected = subprocess.run(command, env=downloaded, capture_output=True, text=True)
        assert rejected.returncode != 0 and "archive checksum mismatch" in rejected.stderr, rejected.stderr
