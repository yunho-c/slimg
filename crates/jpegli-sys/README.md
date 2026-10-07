# slimg-jpegli-sys

The Slimg C shim around the official [google/jpegli](https://github.com/google/jpegli)
repository. It exposes RGB JPEG encoding, RGBA decoding, and result destruction;
Rust never owns libjpeg structs or participates in `setjmp`/`longjmp` handling.
The selected upstream revisions are in `upstream.json` and the Git submodule.
This crate does not depend on libjxl.

## Build selection

The build script selects the first available route:

1. `JPEGLI_SYS_DIR`: an absolute directory containing the validated prebuilt layout below.
2. With the default `vendored` feature, the pinned `jpegli` submodule or an
   explicit `JPEGLI_SYS_SOURCE_DIR` checkout. Source builds need Git, CMake and a
   C++17 compiler. The checkout and required submodules must match `upstream.json`.
3. Download the target/CRT-specific archive and SHA-256 sidecar from the immutable
   `jpegli-prebuilt-v<crate-version>` release in `yunho-c/slimg`.

`DOCS_RS` skips native linking for documentation. `JPEGLI_SYS_DIR` is independent
of `LIBJXL_SYS_DIR`: JXL and Jpegli can each use source or prebuilt libraries.
Downloads require curl and tar. Use the explicit directory for offline builds.
Native releases must exist before publishing this crate; a missing release is
an error, not a silent switch to another codec.

Initialize only the dependencies needed for a source build:

```sh
git submodule update --init crates/jpegli-sys/jpegli
git -C crates/jpegli-sys/jpegli submodule update --init \
  third_party/highway third_party/libjpeg-turbo third_party/skcms
cargo run -p slimg-jpegli-sys --example smoke
```

The parent CMake project builds upstream `jpegli-static`, not the `libjpeg.so`
replacement. The shim preserves the `jpegli_*` API. Highway's namespace is
renamed to `slimg_jpegli_hwy_v1` in both Jpegli and Highway; its archive name is
also private. This permits linking the separate libjxl Highway version without
depending on native library search order. Keep the symbol isolation and shim
ABI synchronized when updating the upstream pin.

## Artifact contract

Archives are named `jpegli-prebuilt-<Rust target>-<CRT>.tar.gz`, with one root
directory of the same name. Supported download targets:

| Target | CRT suffix | Build baseline |
| --- | --- | --- |
| `aarch64-apple-darwin` | `native` | macOS 11.0 |
| `x86_64-apple-darwin` | `native` | macOS 10.13 |
| `x86_64-unknown-linux-gnu` | `native` | Ubuntu 22.04 / glibc 2.35 |
| `aarch64-unknown-linux-gnu` | `native` | Ubuntu 24.04 / glibc 2.39 |
| `x86_64-pc-windows-msvc` | `md` or `mt` | dynamic or static release CRT |

Linux prebuilts target the listed runner baselines, not all Linux distributions.
Use a source build for older systems, musl, or other unsupported targets.
`MACOSX_DEPLOYMENT_TARGET` controls source builds; the loader rejects an artifact
requiring a newer macOS than the consumer requests. Windows artifacts are selected
using Cargo's `crt-static` target feature. Do not mix CRT variants.

```text
manifest.json
build-config.txt
include/jpegli_shim.h
licenses/...
lib/libslimg_jpegli_shim.a
lib/libslimg_jpegli.a
lib/libslimg_jpegli_hwy.a
```

Windows uses corresponding `.lib` filenames without the `lib` prefix. The JSON
manifest records schema and shim ABI versions, crate version, target, CRT,
deployment target, exact upstream revisions, Highway namespace, and library
SHA-256 digests. Both explicit and downloaded directories are validated before
linking. The sidecar checksum detects download corruption; its authenticity
depends on the same trusted HTTPS release as the archive.

## Produce and verify

```sh
cargo build -p slimg-jpegli-sys --release --message-format=json > jpegli-build.jsonl
python3 scripts/package-jpegli.py --build-log jpegli-build.jsonl --output dist
```

Extract the archive, then run `scripts/check-jpegli-package.py --prebuilt DIR
--archive ARCHIVE`. It builds the actual Cargo package without the source tree,
runs the native smoke test and manifest rejection tests, and exercises the
download route with a local HTTP-transport fixture, including a bad checksum.

`build-jpegli-prebuilt.yml` performs those checks for all supported targets and
both Windows CRT variants. Its default behavior uploads CI artifacts. A manual
dispatch with `publish=true` creates an immutable release after verification.
Publish Jpegli artifacts and the JXL 0.1.1 artifacts first, then publish
`slimg-jpegli-sys`, `slimg-libjxl-sys`, `slimg-core`, and the CLI in that order.
