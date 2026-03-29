# Benchmarks

Performance measurements on 512x512 gradient test images using [criterion](https://github.com/bheisler/criterion.rs).

> Environment: Apple M-series, macOS, Rust release profile with thin LTO.
> Results will vary by hardware. Run `cargo bench -p slimg-core` to measure on your machine.

Compression metrics are reported for the same deterministic 512x512 gradient fixture and are fixture-specific. They are intended to complement timing data, not replace it.

## Codec Performance

### Encode (512x512, quality 80)

| Format | Time | Throughput |
|--------|------|-----------|
| QOI | 352 µs | 744 Mpx/s |
| JPEG | 7.2 ms | 36 Mpx/s |
| WebP | 9.3 ms | 28 Mpx/s |
| AVIF | 29 ms | 9 Mpx/s |
| PNG | 45 ms | 5.8 Mpx/s |

## Compression Metrics

The benchmark binaries now emit compact compression tables to stdout and write JSON sidecars to `target/criterion/slimg-metrics/`.

- `codec.json` reports raw-image-to-encoded metrics for codec encode/decode fixtures.
- `pipeline.json` reports encoded-input-to-encoded-output metrics for convert and optimize cases.
- Resize benchmarks remain timing-only.

Formulas:

- `raw_bytes = width * height * 4`
- `encoded_bytes = output.len()`
- `bytes_per_pixel = encoded_bytes / (width * height)`
- `compression_ratio = raw_bytes / encoded_bytes`
- `space_saving_pct = 100 * (1 - encoded_bytes / raw_bytes)`
- `size_change_pct = 100 * (output_bytes / input_bytes - 1)`
- `output_to_input_ratio = output_bytes / input_bytes`

### Decode

| Format | Time | Throughput |
|--------|------|-----------|
| QOI | 254 µs | 980 MiB/s |
| JPEG | 380 µs | 12.7 MiB/s |
| PNG | 676 µs | 2.8 MiB/s |
| AVIF | 945 µs | 3.4 MiB/s |
| WebP | 2.1 ms | 1.2 MiB/s |

## Pipeline Performance

### Format Conversion

| Pipeline | Time | Throughput |
|----------|------|-----------|
| PNG → JPEG | 7.8 ms | 33.5 Mpx/s |
| JPEG → WebP | 8.3 ms | 31.6 Mpx/s |
| PNG → AVIF | 28.6 ms | 9.2 Mpx/s |
| WebP → PNG | 203 ms | 1.3 Mpx/s |

### Optimize (re-encode same format)

| Format | Time |
|--------|------|
| WebP | 7.2 ms |
| JPEG | 8.4 ms |
| AVIF | 26 ms |
| PNG | 44 ms |

### Resize (512x512 → 256x256)

| Mode | Time | Throughput |
|------|------|-----------|
| Width | 2.09 ms | 125 Mpx/s |
| Height | 2.09 ms | 125 Mpx/s |
| Exact | 2.09 ms | 125 Mpx/s |
| Scale (0.5x) | 2.08 ms | 126 Mpx/s |
| Fit | 2.09 ms | 125 Mpx/s |

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench -p slimg-core

# Run specific group
cargo bench -p slimg-core -- encode
cargo bench -p slimg-core -- decode
cargo bench -p slimg-core -- convert
cargo bench -p slimg-core -- optimize
cargo bench -p slimg-core -- resize
```

HTML reports are generated at `target/criterion/report/index.html`.

Compression metric JSON files are generated at `target/criterion/slimg-metrics/`.

## Natural Image Corpus

There is also a separate benchmark target for real-image storage/compression workloads based on the Kodak Lossless True Color Image Suite:

```bash
cargo bench -p slimg-core --bench natural_bench
```

By default it looks for the corpus at:

```text
~/GitHub/Kodak-Lossless-True-Color-Image-Suite/PhotoCD_PCD0992
```

You can override that location with:

```bash
SLIMG_BENCH_NATURAL_DIR=/path/to/PhotoCD_PCD0992 \
  cargo bench -p slimg-core --bench natural_bench
```

`natural_bench` benchmarks:

- `encode` with corpus-level compression metrics
- `convert` with corpus-level input/output size change metrics
- `optimize` with corpus-level input/output size change metrics

It intentionally does not benchmark `decode` or `resize`, so it stays focused on real-image storage efficiency rather than general runtime coverage.

Like the synthetic metric benches, it writes a JSON sidecar under `target/criterion/slimg-metrics/` as `natural.json`.

### Natural Resolution Sweep

There is also a standalone analysis target that resizes the natural-image corpus to a set of common max dimensions, encodes the whole corpus with each codec, and writes both a JSON report and an SVG line chart:

```bash
cargo run --release -p slimg-core --example natural_resolution_sweep
```

Or via `just`:

```bash
just bench-natural-sweep
```

Outputs:

- `target/criterion/slimg-metrics/natural_resolution_sweep.json`
- `target/criterion/slimg-metrics/natural_resolution_sweep.svg`

Defaults:

- max dimensions: `128,256,384,512,640,768`
- quality: `80`
- repeats per point: `3`

Optional environment variables:

- `SLIMG_BENCH_NATURAL_DIR=/path/to/PhotoCD_PCD0992`
- `SLIMG_SWEEP_MAX_DIMS=128,256,512,768`
- `SLIMG_SWEEP_QUALITY=80`
- `SLIMG_SWEEP_REPEATS=5`
