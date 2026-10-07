#!/usr/bin/env python3
"""Exercise JPEG and JXL together using only their explicit prebuilt routes."""
import argparse
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--build-log", type=Path, required=True)
args = parser.parse_args()
env = dict(os.environ)
for line in args.build_log.read_text().splitlines():
    message = json.loads(line)
    if message.get("reason") != "build-script-executed":
        continue
    if "slimg-libjxl-sys" in message["package_id"]:
        env["LIBJXL_SYS_DIR"] = str(Path(message["out_dir"]).resolve())
    if "slimg-jpegli-sys" in message["package_id"]:
        env["JPEGLI_SYS_DIR"] = str((Path(message["out_dir"]) / "native").resolve())
assert "LIBJXL_SYS_DIR" in env and "JPEGLI_SYS_DIR" in env, "source build outputs missing"
command = ["cargo", "test", "-p", "slimg-core", "--no-default-features", "--features", "jpeg-backend-jpegli", "--test", "native_coexistence"]
for source in [None, "JPEGLI_SYS_DIR", "LIBJXL_SYS_DIR"]:
    configuration = dict(env)
    if source:
        configuration.pop(source)
    print(f"Native consumer: source={source or 'none'}, other codecs prebuilt", flush=True)
    subprocess.run(command, env=configuration, check=True)
