# JPEGli Integration Implementation Plan

## Overview

Add `jpegli` support to `slimg` as an optional JPEG backend, while keeping the current public `slimg-core` pipeline API and CLI behavior stable.

This should be treated as a backend integration project, not a user-facing feature redesign.
The initial goal is to make `jpegli` a compile-time-selectable replacement for the current `mozjpeg` JPEG implementation.

## Recommended Direction

Use mutually exclusive Cargo features for JPEG backend selection:

- default: `mozjpeg`
- optional: `jpegli`

Do not attempt to link `mozjpeg` and `jpegli`'s libjpeg-compatible wrapper into the same binary.

Why:
- `jpegli`'s wrapper exports standard libjpeg entry points such as `jpeg_CreateCompress`
- `mozjpeg` also provides libjpeg-compatible symbols
- dual-linking both stacks in one binary is likely to create duplicate-symbol or ambiguous-link behavior

Therefore, backend selection should happen at compile time, not at runtime, in v1.

## Goals

1. Add a `jpegli` backend without changing the public `slimg-core` pipeline API.
2. Keep current format detection, resize/crop/extend flow, and output semantics unchanged.
3. Preserve cross-platform support for Linux, macOS, and Windows.
4. Keep the existing `mozjpeg` backend as the default and low-risk path.
5. Make benchmarking and comparison against `mozjpeg` straightforward.

## Non-Goals

- Runtime backend switching
- Exposing JPEG-specific advanced knobs in the public API
- Linking both JPEG backends into the same artifact
- Reworking all image codecs around a new abstraction
- Replacing JXL infrastructure or rethinking `slimg-libjxl-sys`

## Key Design Decision

The safest implementation path is:

1. Keep JPEG backend choice behind Cargo features.
2. Keep `slimg-core`'s public `EncodeOptions` unchanged in v1.
3. Extend `slimg-libjxl-sys` to optionally build `jpegli`.
4. Add a small native shim around `jpegli` rather than binding the raw libjpeg API directly in Rust.

The shim is important because libjpeg-style APIs use callback-driven error handling and `setjmp`/`longjmp`.
That is a poor surface for direct Rust FFI.
A narrow native shim can translate jpegli failures into explicit status codes and owned output buffers.

## Proposed Architecture

### 1. `slimg-core` owns backend selection

`crates/slimg-core/Cargo.toml` should define mutually exclusive JPEG backend features:

```toml
[features]
default = ["jpeg-backend-mozjpeg"]
jpeg-backend-mozjpeg = ["dep:mozjpeg"]
jpeg-backend-jpegli = ["slimg-libjxl-sys/jpegli"]
```

Rules:
- exactly one JPEG backend must be enabled
- `mozjpeg` becomes optional
- `jpeg-backend-mozjpeg` remains the default

Enforce with `compile_error!` guards in the JPEG codec module:
- error if both are enabled
- error if neither is enabled

### 2. `slimg-libjxl-sys` optionally builds jpegli

`crates/libjxl-sys` already vendors and packages `libjxl`.
Reuse that source tree and build pipeline rather than introducing a second vendored libjxl copy.

Add a `jpegli` feature to `crates/libjxl-sys/Cargo.toml`.

When enabled:
- set `JPEGXL_ENABLE_JPEGLI=ON`
- build the jpegli wrapper library needed by the shim
- compile and link a local shim library
- expose only the shim ABI to Rust

### 3. Native shim provides a narrow C ABI

Add a small shim, for example:

```text
crates/libjxl-sys/
├── shim/
│   ├── jpegli_shim.h
│   └── jpegli_shim.cc
```

The shim should expose only the operations `slimg` needs:

- encode packed RGB bytes to JPEG
- decode JPEG bytes to RGBA
- free output buffers allocated by the shim
- return error messages without throwing across FFI

Example shape:

```c
typedef struct {
  uint8_t* data;
  size_t len;
  uint32_t width;
  uint32_t height;
  int status;
  const char* error_message;
} slimg_jpegli_result;

int slimg_jpegli_encode_rgb(
    const uint8_t* rgb,
    uint32_t width,
    uint32_t height,
    uint8_t quality,
    slimg_jpegli_result* out);

int slimg_jpegli_decode_rgba(
    const uint8_t* data,
    size_t len,
    slimg_jpegli_result* out);

void slimg_jpegli_free_result(slimg_jpegli_result* result);
```

The shim should internally:
- own all libjpeg/jpegli structs
- install error handlers
- contain `setjmp`/`longjmp` handling inside native code
- never unwind or longjmp across Rust frames

### 4. `slimg-core` keeps one `JpegCodec` facade

Refactor `crates/slimg-core/src/codec/jpeg.rs` into:

```text
crates/slimg-core/src/codec/jpeg/
├── mod.rs
├── mozjpeg.rs
└── jpegli.rs
```

`mod.rs` exposes the existing `JpegCodec`.
Backend-specific modules implement the same internal encode/decode behavior.

This keeps the rest of the pipeline unchanged:
- [pipeline.rs](/Users/yunhocho/GitHub/slimg/crates/slimg-core/src/pipeline.rs)
- [codec/mod.rs](/Users/yunhocho/GitHub/slimg/crates/slimg-core/src/codec/mod.rs)

## Implementation Tasks

### Task 1: Add backend feature layout

**Files:**
- Modify: `crates/slimg-core/Cargo.toml`
- Modify: `crates/slimg-core/src/codec/jpeg.rs` or split into `codec/jpeg/`

**Work:**
- make `mozjpeg` optional
- add mutually exclusive backend features
- add compile-time guards
- keep default behavior unchanged for existing users

**Success criteria:**
- `cargo build -p slimg-core` still uses `mozjpeg` by default
- `cargo build -p slimg-core --no-default-features --features jpeg-backend-jpegli` selects the new backend

### Task 2: Extend `slimg-libjxl-sys` for jpegli

**Files:**
- Modify: `crates/libjxl-sys/Cargo.toml`
- Modify: `crates/libjxl-sys/build.rs`
- Create: `crates/libjxl-sys/shim/jpegli_shim.h`
- Create: `crates/libjxl-sys/shim/jpegli_shim.cc`
- Optionally create: `crates/libjxl-sys/src/jpegli.rs`

**Work:**
- add `jpegli` feature
- add `cc` build dependency if needed for compiling the shim
- teach `build.rs` to:
  - enable jpegli in the vendored build when feature is on
  - include jpegli headers from the built tree
  - compile the shim
  - link jpegli-related native libs and the shim
- keep the existing non-jpegli path unchanged

**Important constraint:**
- do not expose the full libjpeg ABI to Rust unless the shim approach proves unworkable

**Success criteria:**
- `cargo build -p slimg-libjxl-sys --features jpegli` succeeds on supported platforms
- the generated or handwritten Rust FFI layer only touches shim symbols

### Task 3: Implement `slimg-core` jpegli codec backend

**Files:**
- Create: `crates/slimg-core/src/codec/jpeg/mod.rs`
- Create: `crates/slimg-core/src/codec/jpeg/mozjpeg.rs`
- Create: `crates/slimg-core/src/codec/jpeg/jpegli.rs`
- Modify: `crates/slimg-core/src/codec/mod.rs`

**Work:**
- move current `mozjpeg` logic into `mozjpeg.rs`
- implement a `jpegli` backend using the shim ABI
- keep:
  - `ImageData` contract
  - `EncodeOptions { quality: u8 }`
  - existing `Format::Jpeg` routing
- map shim failures to existing `Error::Decode` and `Error::Encode`

**Important behavior rule:**
- if `jpeg-backend-jpegli` is enabled, both JPEG encode and decode should come from `jpegli`
- do not mix `mozjpeg` decode with `jpegli` encode in the same binary

**Success criteria:**
- JPEG roundtrip tests pass under both backend feature sets
- invalid JPEG input returns structured errors, not crashes

### Task 4: Prebuilt and CI plumbing

**Files:**
- Modify: `.github/workflows/build-libjxl-prebuilt.yml`
- Modify: any CI workflow that builds/tests `slimg-core`
- Optionally add: a backend-matrix CI job

**Work:**
- decide how prebuilt artifacts are versioned:
  - either bundle jpegli assets into the existing `libjxl` prebuilt artifacts
  - or publish separate feature-specific archives
- ensure CI exercises both:
  - default `mozjpeg` build
  - `jpegli` build

**Recommended CI commands:**

```bash
cargo test --workspace
cargo test -p slimg-core --no-default-features --features jpeg-backend-jpegli
```

**Success criteria:**
- both backend configurations build in CI
- prebuilt consumption works without ad hoc local setup

### Task 5: Benchmark and evaluate quality/performance

**Files:**
- Modify: `crates/slimg-core/benches/codec_bench.rs`
- Optionally add: `crates/slimg-core/benches/jpeg_backend_bench.rs`
- Optionally add: corpus comparison scripts under `scripts/`

**Work:**
- compare `mozjpeg` and `jpegli` on:
  - output size
  - encode speed
  - decode speed
  - representative image corpus quality
- keep the first benchmark pass focused on practical app inputs:
  - photos
  - screenshots/UI
  - synthetic test gradients

**Decision point after benchmarking:**
- keep `jpegli` experimental only
- or promote it to a supported alternative backend
- or eventually switch the default

### Task 6: Documentation

**Files:**
- Modify: `README.md`
- Modify: `crates/slimg-core/README.md` if present
- Optionally add: `docs/jpeg-backends.md`

**Work:**
- document Cargo features
- document that backend selection is compile-time
- document the current default backend
- document that `jpegli` is experimental until benchmarks and cross-platform validation are complete

## Validation Matrix

At minimum, verify:

- Linux x86_64
- macOS Apple Silicon
- macOS Intel
- Windows x86_64

For each platform:
- build default backend
- build `jpegli` backend
- run JPEG roundtrip tests
- run one file-level pipeline test that touches decode, transform, and encode

## Tests To Add

### Unit tests

- backend feature guard tests if practical
- jpeg quality monotonicity under the `jpegli` backend
- decode error mapping for corrupt data
- encode/decode dimension preservation

### Integration tests

- `process_bytes` JPEG roundtrip under `jpegli`
- `process_file` convert/optimize under `jpegli`
- overwrite/non-overwrite file behavior remains unchanged

### Regression tests

- current JPEG tests still pass under default features
- enabling `jpegli` does not affect PNG/WebP/AVIF/JXL/QOI behavior

## Risks

### 1. Native symbol collision

This is the main architecture risk.
Mitigation:
- compile-time mutually exclusive backends
- do not dual-link `mozjpeg` and `jpegli`

### 2. Error-handling mismatch

libjpeg-style APIs rely on native error machinery.
Mitigation:
- use a narrow native shim
- keep `setjmp`/`longjmp` inside native code only

### 3. Prebuilt artifact complexity

The current `slimg-libjxl-sys` release flow packages JXL assets only.
Mitigation:
- explicitly version and test the jpegli-enabled artifact path

### 4. Quality semantic drift

`quality: 85` may not produce the same output characteristics across backends.
Mitigation:
- document the backend difference
- benchmark before changing defaults

## Incremental Delivery

### Phase 1

- feature-gate backend selection in `slimg-core`
- refactor JPEG codec into backend modules
- keep behavior identical under default `mozjpeg`

### Phase 2

- add jpegli build support in `slimg-libjxl-sys`
- add the native shim
- get `slimg-core` compiling with `jpeg-backend-jpegli`

### Phase 3

- add tests and CI matrix coverage
- verify Linux, macOS, Windows builds

### Phase 4

- benchmark against `mozjpeg`
- decide whether `jpegli` remains experimental or becomes a first-class supported backend

## Acceptance Criteria

The integration is complete when:

1. `slimg-core` builds with either JPEG backend selected at compile time.
2. Default builds still use `mozjpeg` and remain behaviorally compatible.
3. `jpegli` builds and passes JPEG encode/decode tests on supported platforms.
4. No binary links both `mozjpeg` and `jpegli` simultaneously.
5. Native error handling is contained inside the shim, not across Rust FFI boundaries.
6. CI covers both backend configurations.
7. The repo documents how to build and test the `jpegli` backend.

## Recommended First PR Split

PR 1:
- feature layout
- JPEG codec refactor into backend modules
- no behavior change

PR 2:
- `slimg-libjxl-sys` jpegli feature
- native shim
- `jpegli` backend implementation

PR 3:
- CI, prebuilt packaging, docs, and benchmarks
