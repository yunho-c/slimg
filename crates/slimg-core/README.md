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
| JPEG XL | Yes | Yes | libjxl; optional experimental GJXL encoder |

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

The `jpegli` backend currently requires the vendored `libjxl` source tree and is not supported through the prebuilt `slimg-libjxl-sys` archive path.

## Experimental GJXL Encoder

On macOS, JPEG XL encoding can opt into GJXL's experimental C API while
retaining libjxl for decoding and unsupported encode requests. During the
experimental local integration, keep GJXL checked out beside Slimg, then
enable the feature:

```bash
cargo test -p slimg-core --features jxl-encoder-gjxl
```

The feature only supports macOS builds. It uses GJXL's `AUTO` execution policy
and falls back to libjxl for lossless, alpha, an explicit thread budget, or an
unsupported/unavailable GJXL request. Other GJXL failures remain errors.

Experiments and benchmarks should call
`slimg_core::codec::jxl::encode_with_diagnostics` and record its backend and
fallback reason. The ordinary `Codec::encode` interface intentionally returns
only the encoded bytes.

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
