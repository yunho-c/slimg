use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use slimg_core::codec::{EncodeOptions, get_codec};
use slimg_core::resize::resize;
use slimg_core::{Format, ImageData, PipelineOptions, ResizeMode, convert, decode_file, optimize};

const NATURAL_BENCH_QUALITY: u8 = 80;
const NATURAL_OPTIMIZE_INPUT_QUALITY: u8 = 90;
const NATURAL_BENCH_ENV: &str = "SLIMG_BENCH_NATURAL_DIR";
const NATURAL_BENCH_DEFAULT_REPO: &str = "GitHub/Kodak-Lossless-True-Color-Image-Suite/PhotoCD_PCD0992";

#[derive(Debug, Clone)]
struct NaturalImage {
    image: ImageData,
}

#[derive(Debug, Clone)]
struct NaturalCorpus {
    source_dir: PathBuf,
    images: Vec<NaturalImage>,
    total_pixels: u64,
}

#[derive(Debug)]
struct ConvertCase {
    label: &'static str,
    decoded_images: Vec<ImageData>,
    options: PipelineOptions,
}

#[derive(Debug)]
struct OptimizeCase {
    label: &'static str,
    encoded_images: Vec<Vec<u8>>,
    total_input_bytes: u64,
}

fn bench_natural(c: &mut Criterion) {
    let Some(corpus) = load_natural_corpus() else {
        return;
    };

    eprintln!(
        "Using natural benchmark corpus from {} ({} images)",
        corpus.source_dir.display(),
        corpus.images.len(),
    );

    bench_encode_natural(c, &corpus);
    bench_decode_natural(c, &corpus);
    bench_convert_natural(c, &corpus);
    bench_optimize_natural(c, &corpus);
    bench_resize_natural(c, &corpus);
}

fn bench_encode_natural(c: &mut Criterion, corpus: &NaturalCorpus) {
    let options = EncodeOptions {
        quality: NATURAL_BENCH_QUALITY,
    };
    let mut group = c.benchmark_group("encode");
    group.throughput(Throughput::Elements(corpus.total_pixels));

    for format in encodable_formats() {
        let codec = get_codec(format);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{:?}", format)),
            &(&corpus.images, &options),
            |b, &(images, options)| {
                b.iter(|| {
                    for image in images {
                        codec.encode(&image.image, options).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_decode_natural(c: &mut Criterion, corpus: &NaturalCorpus) {
    let options = EncodeOptions {
        quality: NATURAL_BENCH_QUALITY,
    };
    let mut group = c.benchmark_group("decode");

    for format in decodable_formats() {
        let codec = get_codec(format);
        let encoded_images = corpus
            .images
            .iter()
            .map(|image| codec.encode(&image.image, &options).unwrap())
            .collect::<Vec<_>>();
        let total_bytes = encoded_images.iter().map(|data| data.len() as u64).sum();

        group.throughput(Throughput::Bytes(total_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{:?}", format)),
            &encoded_images,
            |b, encoded_images| {
                b.iter(|| {
                    for data in encoded_images {
                        codec.decode(data).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_convert_natural(c: &mut Criterion, corpus: &NaturalCorpus) {
    let cases = build_convert_cases(corpus);
    let mut group = c.benchmark_group("convert");
    group.throughput(Throughput::Elements(corpus.total_pixels));

    for case in &cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(case.label),
            &(&case.decoded_images, &case.options),
            |b, &(images, options)| {
                b.iter(|| {
                    for image in images {
                        convert(image, options).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_optimize_natural(c: &mut Criterion, corpus: &NaturalCorpus) {
    let cases = build_optimize_cases(corpus);
    let mut group = c.benchmark_group("optimize");

    for case in &cases {
        group.throughput(Throughput::Bytes(case.total_input_bytes));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.label),
            &case.encoded_images,
            |b, encoded_images| {
                b.iter(|| {
                    for data in encoded_images {
                        optimize(data, NATURAL_BENCH_QUALITY).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_resize_natural(c: &mut Criterion, corpus: &NaturalCorpus) {
    let modes: Vec<(&str, ResizeMode)> = vec![
        ("width_256", ResizeMode::Width(256)),
        ("height_256", ResizeMode::Height(256)),
        ("exact_256x256", ResizeMode::Exact(256, 256)),
        ("scale_0.5", ResizeMode::Scale(0.5)),
        ("fit_256x256", ResizeMode::Fit(256, 256)),
    ];

    let mut group = c.benchmark_group("resize");
    group.throughput(Throughput::Elements(corpus.total_pixels));

    for (name, mode) in &modes {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(&corpus.images, mode),
            |b, &(images, mode)| {
                b.iter(|| {
                    for image in images {
                        resize(&image.image, mode).unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn build_convert_cases(corpus: &NaturalCorpus) -> Vec<ConvertCase> {
    let conversions: Vec<(&str, Format, Format)> = vec![
        ("jpeg_to_webp", Format::Jpeg, Format::WebP),
        ("png_to_jpeg", Format::Png, Format::Jpeg),
        ("webp_to_png", Format::WebP, Format::Png),
        ("png_to_avif", Format::Png, Format::Avif),
    ];

    conversions
        .into_iter()
        .map(|(label, src_format, dst_format)| {
            let decoded_images = corpus
                .images
                .iter()
                .map(|image| {
                    let encoded = encode_image(&image.image, src_format, NATURAL_BENCH_QUALITY);
                    get_codec(src_format).decode(&encoded).unwrap()
                })
                .collect::<Vec<_>>();

            let options = PipelineOptions {
                format: dst_format,
                quality: NATURAL_BENCH_QUALITY,
                resize: None,
                crop: None,
                extend: None,
                fill_color: None,
            };

            ConvertCase {
                label,
                decoded_images,
                options,
            }
        })
        .collect()
}

fn build_optimize_cases(corpus: &NaturalCorpus) -> Vec<OptimizeCase> {
    let formats = vec![
        ("Jpeg", Format::Jpeg),
        ("Png", Format::Png),
        ("WebP", Format::WebP),
        ("Avif", Format::Avif),
    ];

    formats
        .into_iter()
        .map(|(label, format)| {
            let encoded_images = corpus
                .images
                .iter()
                .map(|image| encode_image(&image.image, format, NATURAL_OPTIMIZE_INPUT_QUALITY))
                .collect::<Vec<_>>();
            let total_input_bytes = encoded_images.iter().map(|data| data.len() as u64).sum();

            OptimizeCase {
                label,
                encoded_images,
                total_input_bytes,
            }
        })
        .collect()
}

fn load_natural_corpus() -> Option<NaturalCorpus> {
    let source_dir = match natural_bench_dir() {
        Some(path) => path,
        None => {
            eprintln!(
                "Skipping natural benchmarks: set {NATURAL_BENCH_ENV} or clone the Kodak corpus under ~/{}",
                NATURAL_BENCH_DEFAULT_REPO
            );
            return None;
        }
    };

    let mut paths = match fs::read_dir(&source_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
            .collect::<Vec<_>>(),
        Err(err) => {
            eprintln!(
                "Skipping natural benchmarks: failed to read {}: {err}",
                source_dir.display(),
            );
            return None;
        }
    };
    paths.sort();

    if paths.is_empty() {
        eprintln!(
            "Skipping natural benchmarks: no PNG images found in {}",
            source_dir.display(),
        );
        return None;
    }

    let images = paths
        .iter()
        .map(|path| {
            let (image, format) = decode_file(path).unwrap_or_else(|err| {
                panic!("failed to decode natural benchmark image {}: {err}", path.display())
            });
            assert_eq!(
                format,
                Format::Png,
                "natural benchmark corpus must contain PNG files",
            );

            NaturalImage {
                image,
            }
        })
        .collect::<Vec<_>>();

    let total_pixels = images
        .iter()
        .map(|image| u64::from(image.image.width) * u64::from(image.image.height))
        .sum();

    Some(NaturalCorpus {
        source_dir,
        images,
        total_pixels,
    })
}

fn natural_bench_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os(NATURAL_BENCH_ENV) {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }

    let home = env::var_os("HOME")?;
    let path = Path::new(&home).join(NATURAL_BENCH_DEFAULT_REPO);
    path.is_dir().then_some(path)
}

fn encode_image(image: &ImageData, format: Format, quality: u8) -> Vec<u8> {
    get_codec(format)
        .encode(image, &EncodeOptions { quality })
        .unwrap()
}

fn encodable_formats() -> Vec<Format> {
    vec![Format::Jpeg, Format::Png, Format::WebP, Format::Qoi, Format::Avif]
}

fn decodable_formats() -> Vec<Format> {
    vec![Format::Jpeg, Format::Png, Format::WebP, Format::Qoi, Format::Avif]
}

criterion_group!(benches, bench_natural);
criterion_main!(benches);
