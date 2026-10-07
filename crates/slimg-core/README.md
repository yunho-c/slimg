# slimg-core

Image optimization library for Rust. Decode, encode, convert, and resize images using best-in-class codecs.

## Codecs

| Format | Decode | Encode | Encoder |
|--------|--------|--------|---------|
| JPEG | Yes | Yes | MozJPEG by default; optional `jpeg-backend-jpegli` feature |
| PNG | Yes | Yes | OxiPNG (Zopfli) |
| WebP | Yes | Yes | libwebp |
| AVIF | Yes | Yes | zenavif decode; ravif encode (AV1) |
| QOI | Yes | Yes | rapid-qoi |
| JPEG XL | Yes | Yes | libjxl |

## JPEG Backends

`slimg-core` uses `mozjpeg` by default:

```bash
cargo test -p slimg-core
```

To test or build the experimental `jpegli` backend, disable default features and enable `jpeg-backend-jpegli`:

```bash
git submodule update --init --recursive
cargo test -p slimg-core --no-default-features --features jpeg-backend-jpegli
```

The `jpegli` backend uses the official standalone `google/jpegli` repository through `slimg-jpegli-sys`. Jpegli and JPEG XL have independent source/prebuilt routes: set `JPEGLI_SYS_DIR` and `LIBJXL_SYS_DIR` respectively. See [native build and packaging instructions](../jpegli-sys/README.md) for artifact requirements and release setup.

When also depending on `slimg-exec` or `slimg-ffi`, disable their default features
and select `jpeg-backend-jpegli` there as well. The CLI, executor, FFI and GUI
forward the same mutually exclusive JPEG features. Selecting Jpegli still
selects both encoding and decoding; MozJPEG remains the default.

### Jpegli effort

`EncodeOptions::effort` (also exposed as CLI `--effort`) selects these settings:

| Effort | JPEG scan mode | Huffman tables |
| --- | --- | --- |
| 0–24 | Sequential | Fixed |
| 25–49 | Sequential | Optimized |
| 50–74 | Progressive level 1 | Optimized |
| 75–100 | Progressive level 2 | Optimized |
| Unset (`None`) | Progressive level 2 (existing default) | Optimized |

Values above 100 behave as 100. Higher tiers enable more compression work;
actual speed and size depend on the image, so size need not decrease at every
tier. Effort leaves quality, chroma subsampling and adaptive quantization
unchanged. Jpegli has no internal thread-budget control; `threads` remains unused.

## Usage

```rust
use slimg_core::*;
use std::path::Path;

// Decode from file
let (image, format) = decode_file(Path::new("photo.jpg"))?;

// Convert to WebP at quality 80
let result = convert(&image, &PipelineOptions {
    format: Format::WebP,
    quality: 80,
    effort: None,
    threads: None,
    resize: None,
    crop: None,
    extend: None,
    fill_color: None,
})?;
result.save(Path::new("photo.webp"))?;

// Convert and resize in one step
let result = convert(&image, &PipelineOptions {
    format: Format::Avif,
    quality: 60,
    effort: Some(75),
    threads: None,
    resize: Some(ResizeMode::Width(800)),
    crop: None,
    extend: None,
    fill_color: None,
})?;

// Optimize in-place (re-encode same format)
let data = std::fs::read("photo.jpg")?;
let optimized = optimize_with_options(&data, EncodeOptions {
    quality: 75,
    effort: Some(75),
    threads: None,
})?;
optimized.save(Path::new("photo.jpg"))?;
```

## Resize Modes

| Mode | Description |
|------|-------------|
| `Width(u32)` | Set width, preserve aspect ratio |
| `Height(u32)` | Set height, preserve aspect ratio |
| `Fit(u32, u32)` | Fit within bounds, preserve aspect ratio |
| `Exact(u32, u32)` | Exact dimensions (may distort) |
| `Scale(f64)` | Scale factor (e.g. 0.5 = half size) |

## CLI

For batch processing and command-line usage, see [slimg](https://crates.io/crates/slimg).

## License

MIT
