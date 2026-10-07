#!/usr/bin/env python3
"""Package the exact native output produced by slimg-jpegli-sys's build script."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--build-log", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
outputs = []
for line in args.build_log.read_text().splitlines():
    message = json.loads(line)
    if message.get("reason") == "build-script-executed" and "slimg-jpegli-sys" in message["package_id"]:
        outputs.append(Path(message["out_dir"]) / "native")
if len(outputs) != 1:
    parser.error(f"expected exactly one Jpegli source build output, found {outputs}")
source = outputs[0]
manifest = json.loads((source / "manifest.json").read_text())
name = f"jpegli-prebuilt-{manifest['target']}-{manifest['crt']}"
args.output.mkdir(parents=True, exist_ok=True)
archive = args.output / f"{name}.tar.gz"
with tempfile.TemporaryDirectory() as temporary:
    stage = Path(temporary) / name
    stage.mkdir()
    for entry in ["lib", "include", "licenses"]:
        shutil.copytree(source / entry, stage / entry)
    for entry in ["manifest.json", "build-config.txt"]:
        shutil.copyfile(source / entry, stage / entry)
    for file, digest in manifest["files"].items():
        assert hashlib.sha256((stage / file).read_bytes()).hexdigest() == digest, file
    with tarfile.open(archive, "w:gz") as package:
        package.add(stage, arcname=name)
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n")
print(archive)
