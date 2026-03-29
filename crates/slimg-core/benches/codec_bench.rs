mod support;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_main};
use serde::Serialize;
use slimg_core::{EncodeOptions, Format, codec::get_codec};

use support::{
    BENCH_QUALITY, CompressionRow, benchmark_fixture, compression_metrics, fixture_info,
    print_compression_table, write_json_report,
};

/// Formats that support encoding.
///
/// JXL is excluded because encoding is not supported (license restrictions).
/// AVIF encoding works on all platforms via `ravif`.
fn encodable_formats() -> Vec<Format> {
    vec![Format::Jpeg, Format::Png, Format::WebP, Format::Qoi, Format::Avif]
}

/// Formats that support both encoding and decoding (needed for decode benchmarks).
fn decodable_formats() -> Vec<Format> {
    vec![Format::Jpeg, Format::Png, Format::WebP, Format::Qoi, Format::Avif]
}

#[derive(Debug)]
struct CodecSample {
    format: Format,
    label: &'static str,
    encoded: Vec<u8>,
    metrics: support::CompressionMetrics,
}

#[derive(Debug, Serialize)]
struct CodecMetricsReport {
    fixture: support::FixtureInfo,
    encode_quality: u8,
    entries: Vec<CompressionRow>,
}

fn bench_codec(c: &mut Criterion) {
    let image = benchmark_fixture();
    let fixture = fixture_info(&image).expect("fixture metrics should be valid");
    let options = EncodeOptions {
        quality: BENCH_QUALITY,
    };
    let pixel_count = u64::from(fixture.width) * u64::from(fixture.height);
    let samples = build_codec_samples(&image, &options);
    let report_rows = samples
        .iter()
        .map(|sample| CompressionRow {
            label: sample.label.to_string(),
            metrics: sample.metrics,
        })
        .collect::<Vec<_>>();

    print_compression_table("Codec compression metrics", &fixture, BENCH_QUALITY, &report_rows);
    write_json_report(
        "codec",
        &CodecMetricsReport {
            fixture: fixture.clone(),
            encode_quality: BENCH_QUALITY,
            entries: report_rows,
        },
    )
    .expect("codec metrics JSON should be written");

    let image_and_options = (&image, &options);
    let mut group = c.benchmark_group("encode");
    group.throughput(Throughput::Elements(pixel_count));

    for sample in &samples {
        let codec = get_codec(sample.format);
        group.bench_with_input(
            BenchmarkId::from_parameter(sample.label),
            &image_and_options,
            |b, &(image, options)| {
                b.iter(|| codec.encode(image, options).unwrap());
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("decode");

    for sample in &samples {
        let codec = get_codec(sample.format);
        group.throughput(Throughput::Bytes(sample.encoded.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(sample.label),
            &sample.encoded,
            |b, data| {
                b.iter(|| codec.decode(data).unwrap());
            },
        );
    }
    group.finish();
}

fn build_codec_samples(image: &slimg_core::ImageData, options: &EncodeOptions) -> Vec<CodecSample> {
    encodable_formats()
        .into_iter()
        .filter(|format| decodable_formats().contains(format))
        .map(|format| {
            let codec = get_codec(format);
            let encoded = codec.encode(image, options).expect("sample encode should succeed");
            let metrics = compression_metrics(image, encoded.len())
                .expect("compression metrics should be valid");

            CodecSample {
                format,
                label: format_label(format),
                encoded,
                metrics,
            }
        })
        .collect()
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

support::slimg_criterion_group!(benches, bench_codec);
criterion_main!(benches);
