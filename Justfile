set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Show available recipes.
default:
    @just --list

# Initialize git submodules required for vendored native builds.
submodules:
    git submodule update --init --recursive

# Run the default slimg-core test suite.
test-core:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo test -p slimg-core

# Run the CLI test suite.
test-cli:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo test -p slimg

# Run the slimg-core test suite with the experimental jpegli backend.
test-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo test -p slimg-core --no-default-features --features jpeg-backend-jpegli

# Build the CLI with the default JPEG backend.
build-cli:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo build -p slimg

# Build the CLI with the experimental jpegli backend.
build-cli-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo build -p slimg --no-default-features --features jpeg-backend-jpegli

# Run all slimg-core Criterion benchmarks.
bench:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core

# Run all slimg-core Criterion benchmarks with reduced sample size and timing.
bench-fast:
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core

# Run all slimg-core Criterion benchmarks with the jpegli backend.
bench-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --no-default-features --features jpeg-backend-jpegli

# Run all slimg-core Criterion benchmarks with the jpegli backend and reduced sample size and timing.
bench-fast-jpegli: submodules
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --no-default-features --features jpeg-backend-jpegli

# Run only the codec benchmark target.
bench-codec:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench codec_bench

# Run only the codec benchmark target with reduced sample size and timing.
bench-codec-fast:
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench codec_bench

# Run only the codec benchmark target with the jpegli backend.
bench-codec-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench codec_bench --no-default-features --features jpeg-backend-jpegli

# Run only the codec benchmark target with the jpegli backend and reduced sample size and timing.
bench-codec-fast-jpegli: submodules
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench codec_bench --no-default-features --features jpeg-backend-jpegli

# Run only the pipeline benchmark target.
bench-pipeline:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench pipeline_bench

# Run only the pipeline benchmark target with reduced sample size and timing.
bench-pipeline-fast:
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench pipeline_bench

# Run only the pipeline benchmark target with the jpegli backend.
bench-pipeline-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench pipeline_bench --no-default-features --features jpeg-backend-jpegli

# Run only the pipeline benchmark target with the jpegli backend and reduced sample size and timing.
bench-pipeline-fast-jpegli: submodules
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench pipeline_bench --no-default-features --features jpeg-backend-jpegli

# Run only the natural-image benchmark target.
bench-natural:
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench natural_bench

# Run only the natural-image benchmark target with reduced sample size and timing.
bench-natural-fast:
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench natural_bench

# Run only the natural-image benchmark target with the jpegli backend.
bench-natural-jpegli: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench natural_bench --no-default-features --features jpeg-backend-jpegli

# Run only the natural-image benchmark target with the jpegli backend and reduced sample size and timing.
bench-natural-fast-jpegli: submodules
    SLIMG_BENCH_QUICK=1 SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo bench -p slimg-core --bench natural_bench --no-default-features --features jpeg-backend-jpegli

# Build and run Python binding tests.
test-python: submodules
    cd bindings/python && python -m pip install maturin pytest && maturin build --out dist && python -m pip install dist/*.whl && pytest tests -v

# Build and run Kotlin binding tests.
test-kotlin: submodules
    SYSTEM_DEPS_DAV1D_BUILD_INTERNAL=always cargo build --release -p slimg-ffi
    cargo run -p slimg-ffi --bin uniffi-bindgen generate --library target/release/$(if [[ "$OSTYPE" == darwin* ]]; then echo libslimg_ffi.dylib; elif [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]]; then echo slimg_ffi.dll; else echo libslimg_ffi.so; fi) --language kotlin --out-dir bindings/kotlin/src/main/kotlin
    chmod +x bindings/scripts/patch-generated-kotlin.sh
    bindings/scripts/patch-generated-kotlin.sh bindings/kotlin/src/main/kotlin/io/clroot/slimg/slimg_ffi.kt
    cd bindings/kotlin && ./gradlew test
