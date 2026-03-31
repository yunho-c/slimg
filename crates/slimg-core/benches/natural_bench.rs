mod support;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_main};
use serde::Serialize;
use slimg_core::codec::{EncodeOptions, get_codec};
use slimg_core::{Format, ImageData, PipelineOptions, convert, decode_file, optimize};

use support::{
    CompressionMetrics, CompressionRow, SizeChangeRow, size_change_metrics, write_json_report,
};

const NATURAL_BENCH_QUALITY: u8 = 80;
const NATURAL_OPTIMIZE_INPUT_QUALITY: u8 = 90;
const NATURAL_BENCH_ENV: &str = "SLIMG_BENCH_NATURAL_DIR";
const NATURAL_BENCH_DEFAULT_REPO: &str =
    "GitHub/Kodak-Lossless-True-Color-Image-Suite/PhotoCD_PCD0992";

#[derive(Debug, Clone)]
struct NaturalImage {
    image: ImageData,
}

#[derive(Debug, Clone, Serialize)]
struct NaturalCorpusInfo {
    name: String,
    image_count: usize,
    width: u32,
    height: u32,
    raw_bytes_per_image: u64,
    total_raw_bytes: u64,
    total_pixels: u64,
}

#[derive(Debug, Clone)]
struct NaturalCorpus {
    source_dir: PathBuf,
    images: Vec<NaturalImage>,
    info: NaturalCorpusInfo,
}

#[derive(Debug)]
struct EncodeCase {
    format: Format,
    label: &'static str,
    metrics: CompressionMetrics,
}

#[derive(Debug)]
struct ConvertCase {
    label: &'static str,
    decoded_images: Vec<ImageData>,
    options: PipelineOptions,
    metrics: support::SizeChangeMetrics,
}

#[derive(Debug)]
struct OptimizeCase {
    label: &'static str,
    encoded_images: Vec<Vec<u8>>,
    total_input_bytes: u64,
    metrics: support::SizeChangeMetrics,
}

#[derive(Debug, Serialize)]
struct NaturalMetricsReport {
    corpus: NaturalCorpusInfo,
    encode_quality: u8,
    optimize_input_quality: u8,
    encode: Vec<CompressionRow>,
    convert: Vec<SizeChangeRow>,
    optimize: Vec<SizeChangeRow>,
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

    let encode_cases = build_encode_cases(&corpus);
    let convert_cases = build_convert_cases(&corpus);
    let optimize_cases = build_optimize_cases(&corpus);

    let encode_rows = encode_cases
        .iter()
        .map(|case| CompressionRow {
            label: case.label.to_string(),
            metrics: case.metrics,
        })
        .collect::<Vec<_>>();
    let convert_rows = convert_cases
        .iter()
        .map(|case| SizeChangeRow {
            label: case.label.to_string(),
            metrics: case.metrics,
        })
        .collect::<Vec<_>>();
    let optimize_rows = optimize_cases
        .iter()
        .map(|case| SizeChangeRow {
            label: case.label.to_string(),
            metrics: case.metrics,
        })
        .collect::<Vec<_>>();

    print_natural_compression_table(&corpus.info, &encode_rows);
    print_natural_size_change_table("Natural convert size metrics", &corpus.info, &convert_rows);
    print_natural_size_change_table(
        "Natural optimize size metrics",
        &corpus.info,
        &optimize_rows,
    );
    write_json_report(
        "natural",
        &NaturalMetricsReport {
            corpus: corpus.info.clone(),
            encode_quality: NATURAL_BENCH_QUALITY,
            optimize_input_quality: NATURAL_OPTIMIZE_INPUT_QUALITY,
            encode: encode_rows,
            convert: convert_rows,
            optimize: optimize_rows,
        },
    )
    .expect("natural metrics JSON should be written");

    bench_encode_natural(c, &corpus, &encode_cases);
    bench_convert_natural(c, &corpus, &convert_cases);
    bench_optimize_natural(c, &optimize_cases);
}

fn bench_encode_natural(c: &mut Criterion, corpus: &NaturalCorpus, encode_cases: &[EncodeCase]) {
    let options = EncodeOptions {
        quality: NATURAL_BENCH_QUALITY,
        threads: None,
    };
    let mut group = c.benchmark_group("encode");
    group.throughput(Throughput::Elements(corpus.info.total_pixels));

    for case in encode_cases {
        let codec = get_codec(case.format);
        group.bench_with_input(
            BenchmarkId::from_parameter(case.label),
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

fn bench_convert_natural(c: &mut Criterion, corpus: &NaturalCorpus, cases: &[ConvertCase]) {
    let mut group = c.benchmark_group("convert");
    group.throughput(Throughput::Elements(corpus.info.total_pixels));

    for case in cases {
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

fn bench_optimize_natural(c: &mut Criterion, cases: &[OptimizeCase]) {
    let mut group = c.benchmark_group("optimize");

    for case in cases {
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

fn build_encode_cases(corpus: &NaturalCorpus) -> Vec<EncodeCase> {
    encodable_formats()
        .into_iter()
        .map(|format| {
            let codec = get_codec(format);
            let total_encoded_bytes = corpus
                .images
                .iter()
                .map(|image| {
                    codec
                        .encode(
                            &image.image,
                            &EncodeOptions {
                                quality: NATURAL_BENCH_QUALITY,
                                threads: None,
                            },
                        )
                        .unwrap()
                        .len() as u64
                })
                .sum::<u64>();
            let metrics = aggregate_compression_metrics(&corpus.info, total_encoded_bytes);

            EncodeCase {
                format,
                label: format_label(format),
                metrics,
            }
        })
        .collect()
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
            let mut total_input_bytes = 0usize;
            let mut total_output_bytes = 0usize;
            let decoded_images = corpus
                .images
                .iter()
                .map(|image| {
                    let encoded = encode_image(&image.image, src_format, NATURAL_BENCH_QUALITY);
                    total_input_bytes += encoded.len();
                    let decoded = get_codec(src_format).decode(&encoded).unwrap();
                    let output = convert(
                        &decoded,
                        &PipelineOptions {
                            format: dst_format,
                            quality: NATURAL_BENCH_QUALITY,
                            threads: None,
                            resize: None,
                            crop: None,
                            extend: None,
                            fill_color: None,
                        },
                    )
                    .unwrap();
                    total_output_bytes += output.data.len();
                    decoded
                })
                .collect::<Vec<_>>();

            let options = PipelineOptions {
                format: dst_format,
                quality: NATURAL_BENCH_QUALITY,
                threads: None,
                resize: None,
                crop: None,
                extend: None,
                fill_color: None,
            };
            let metrics = size_change_metrics(total_input_bytes, total_output_bytes)
                .expect("aggregate convert metrics should be valid");

            ConvertCase {
                label,
                decoded_images,
                options,
                metrics,
            }
        })
        .collect()
}

fn build_optimize_cases(corpus: &NaturalCorpus) -> Vec<OptimizeCase> {
    let formats = vec![
        ("jpeg", Format::Jpeg),
        ("png", Format::Png),
        ("webp", Format::WebP),
        ("avif", Format::Avif),
    ];

    formats
        .into_iter()
        .map(|(label, format)| {
            let encoded_images = corpus
                .images
                .iter()
                .map(|image| encode_image(&image.image, format, NATURAL_OPTIMIZE_INPUT_QUALITY))
                .collect::<Vec<_>>();
            let total_input_bytes = encoded_images.iter().map(|data| data.len()).sum::<usize>();
            let total_output_bytes = encoded_images
                .iter()
                .map(|data| optimize(data, NATURAL_BENCH_QUALITY).unwrap().data.len())
                .sum::<usize>();
            let metrics = size_change_metrics(total_input_bytes, total_output_bytes)
                .expect("aggregate optimize metrics should be valid");

            OptimizeCase {
                label,
                encoded_images,
                total_input_bytes: total_input_bytes as u64,
                metrics,
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
                panic!(
                    "failed to decode natural benchmark image {}: {err}",
                    path.display()
                )
            });
            assert_eq!(
                format,
                Format::Png,
                "natural benchmark corpus must contain PNG files"
            );
            NaturalImage { image }
        })
        .collect::<Vec<_>>();

    let first_width = images[0].image.width;
    let first_height = images[0].image.height;
    let raw_bytes_per_image = u64::from(first_width) * u64::from(first_height) * 4;
    let total_pixels = images
        .iter()
        .map(|image| u64::from(image.image.width) * u64::from(image.image.height))
        .sum::<u64>();
    let total_raw_bytes = images.len() as u64 * raw_bytes_per_image;

    Some(NaturalCorpus {
        source_dir,
        images,
        info: NaturalCorpusInfo {
            name: "Kodak Lossless True Color Image Suite".to_string(),
            image_count: paths.len(),
            width: first_width,
            height: first_height,
            raw_bytes_per_image,
            total_raw_bytes,
            total_pixels,
        },
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

fn aggregate_compression_metrics(
    info: &NaturalCorpusInfo,
    encoded_bytes: u64,
) -> CompressionMetrics {
    CompressionMetrics {
        raw_bytes: info.total_raw_bytes,
        encoded_bytes,
        bytes_per_pixel: encoded_bytes as f64 / info.total_pixels as f64,
        compression_ratio: info.total_raw_bytes as f64 / encoded_bytes as f64,
        space_saving_pct: 100.0 * (1.0 - encoded_bytes as f64 / info.total_raw_bytes as f64),
    }
}

fn print_natural_compression_table(corpus: &NaturalCorpusInfo, rows: &[CompressionRow]) {
    println!();
    println!("Natural encode compression metrics");
    println!(
        "Corpus: {} ({} images, {}x{}, total raw {} bytes, quality {})",
        corpus.name,
        corpus.image_count,
        corpus.width,
        corpus.height,
        corpus.total_raw_bytes,
        NATURAL_BENCH_QUALITY
    );
    print_compression_table_rows(rows);
}

fn print_natural_size_change_table(
    title: &str,
    corpus: &NaturalCorpusInfo,
    rows: &[SizeChangeRow],
) {
    println!();
    println!("{title}");
    println!(
        "Corpus: {} ({} images, {}x{}, total raw {} bytes)",
        corpus.name, corpus.image_count, corpus.width, corpus.height, corpus.total_raw_bytes
    );
    print_size_change_table_rows(rows);
}

fn print_compression_table_rows(rows: &[CompressionRow]) {
    println!(
        "{:<22} {:>12} {:>13} {:>8} {:>8}",
        "Format", "Encoded bytes", "BPP", "Ratio", "Saving"
    );
    println!("{}", "-".repeat(70));

    for row in rows {
        println!(
            "{:<22} {:>12} {:>13.4} {:>8.2}x {:>7.2}%",
            row.label,
            row.metrics.encoded_bytes,
            row.metrics.bytes_per_pixel,
            row.metrics.compression_ratio,
            row.metrics.space_saving_pct,
        );
    }
}

fn print_size_change_table_rows(rows: &[SizeChangeRow]) {
    println!(
        "{:<22} {:>12} {:>13} {:>8} {:>8}",
        "Case", "Input bytes", "Output bytes", "Out/In", "Change"
    );
    println!("{}", "-".repeat(70));

    for row in rows {
        println!(
            "{:<22} {:>12} {:>13} {:>8.4} {:>7.2}%",
            row.label,
            row.metrics.input_bytes,
            row.metrics.output_bytes,
            row.metrics.output_to_input_ratio,
            row.metrics.size_change_pct,
        );
    }
}

fn encodable_formats() -> Vec<Format> {
    vec![
        Format::Jpeg,
        Format::Png,
        Format::WebP,
        Format::Qoi,
        Format::Avif,
    ]
}

fn format_label(format: Format) -> &'static str {
    match format {
        Format::Jpeg => "JPEG",
        Format::Png => "PNG",
        Format::WebP => "WebP",
        Format::Avif => "AVIF",
        Format::Jxl => "JXL",
        Format::Qoi => "QOI",
    }
}

support::slimg_criterion_group!(benches, bench_natural);
criterion_main!(benches);
