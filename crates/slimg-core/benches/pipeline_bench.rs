mod support;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_main};
use serde::Serialize;
use slimg_core::codec::{EncodeOptions, get_codec};
use slimg_core::resize::resize;
use slimg_core::{Format, ImageData, PipelineOptions, ResizeMode, convert, optimize};

use support::{
    BENCH_QUALITY, SizeChangeRow, benchmark_fixture, fixture_info, print_size_change_table,
    size_change_metrics, write_json_report,
};

/// Pre-encode a test image in the given format and return the encoded bytes.
fn pre_encode(image: &ImageData, format: Format, quality: u8) -> Vec<u8> {
    let codec = get_codec(format);
    let options = EncodeOptions {
        quality,
        effort: None,
        png_palette: Default::default(),
        threads: None,
    };
    codec.encode(image, &options).unwrap()
}

#[derive(Debug)]
struct ConvertCase {
    label: &'static str,
    decoded: ImageData,
    options: PipelineOptions,
    metrics: support::SizeChangeMetrics,
}

#[derive(Debug)]
struct OptimizeCase {
    label: &'static str,
    input_bytes: Vec<u8>,
    metrics: support::SizeChangeMetrics,
}

#[derive(Debug, Serialize)]
struct PipelineMetricsReport {
    fixture: support::FixtureInfo,
    convert: Vec<SizeChangeRow>,
    optimize: Vec<SizeChangeRow>,
}

fn bench_pipeline(c: &mut Criterion) {
    let image = benchmark_fixture();
    let fixture = fixture_info(&image).expect("fixture metrics should be valid");
    let pixel_count = u64::from(fixture.width) * u64::from(fixture.height);
    let convert_cases = build_convert_cases(&image);
    let optimize_cases = build_optimize_cases(&image);
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

    print_size_change_table(
        "Pipeline convert compression metrics",
        &fixture,
        &convert_rows,
    );
    print_size_change_table(
        "Pipeline optimize compression metrics",
        &fixture,
        &optimize_rows,
    );
    write_json_report(
        "pipeline",
        &PipelineMetricsReport {
            fixture: fixture.clone(),
            convert: convert_rows,
            optimize: optimize_rows,
        },
    )
    .expect("pipeline metrics JSON should be written");

    let mut group = c.benchmark_group("convert");
    group.throughput(Throughput::Elements(pixel_count));

    for case in &convert_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(case.label),
            &(&case.decoded, &case.options),
            |b, &(image, opts)| {
                b.iter(|| convert(image, opts).unwrap());
            },
        );
    }

    group.finish();

    let mut group = c.benchmark_group("optimize");

    for case in &optimize_cases {
        group.throughput(Throughput::Bytes(case.input_bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.label),
            &case.input_bytes,
            |b, data| {
                b.iter(|| optimize(data, BENCH_QUALITY).unwrap());
            },
        );
    }

    group.finish();

    let modes: Vec<(&str, ResizeMode)> = vec![
        ("width_256", ResizeMode::Width(256)),
        ("height_256", ResizeMode::Height(256)),
        ("exact_256x256", ResizeMode::Exact(256, 256)),
        ("scale_0.5", ResizeMode::Scale(0.5)),
        ("fit_256x256", ResizeMode::Fit(256, 256)),
    ];

    let mut group = c.benchmark_group("resize");
    group.throughput(Throughput::Elements(pixel_count));

    for (name, mode) in &modes {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(&image, mode),
            |b, &(image, mode)| {
                b.iter(|| resize(image, mode).unwrap());
            },
        );
    }

    group.finish();
}

fn build_convert_cases(image: &ImageData) -> Vec<ConvertCase> {
    let conversions: Vec<(&str, Format, Format)> = vec![
        ("jpeg_to_webp", Format::Jpeg, Format::WebP),
        ("png_to_jpeg", Format::Png, Format::Jpeg),
        ("webp_to_png", Format::WebP, Format::Png),
        ("png_to_avif", Format::Png, Format::Avif),
    ];

    conversions
        .into_iter()
        .map(|(label, src_format, dst_format)| {
            let input_bytes = pre_encode(image, src_format, BENCH_QUALITY);
            let decoded = get_codec(src_format)
                .decode(&input_bytes)
                .expect("sample decode should succeed");
            let options = PipelineOptions {
                format: dst_format,
                quality: BENCH_QUALITY,
                effort: None,
                png_palette: Default::default(),
                threads: None,
                resize: None,
                crop: None,
                extend: None,
                fill_color: None,
            };
            let output_bytes = convert(&decoded, &options)
                .expect("sample convert should succeed")
                .data;
            let metrics = size_change_metrics(input_bytes.len(), output_bytes.len())
                .expect("size change metrics should be valid");

            ConvertCase {
                label,
                decoded,
                options,
                metrics,
            }
        })
        .collect()
}

fn build_optimize_cases(image: &ImageData) -> Vec<OptimizeCase> {
    let formats = vec![
        ("Jpeg", Format::Jpeg),
        ("Png", Format::Png),
        ("WebP", Format::WebP),
        ("Avif", Format::Avif),
    ];

    formats
        .into_iter()
        .map(|(label, format)| {
            let input_bytes = pre_encode(image, format, 90);
            let output_bytes = optimize(&input_bytes, BENCH_QUALITY)
                .expect("sample optimize should succeed")
                .data;
            let metrics = size_change_metrics(input_bytes.len(), output_bytes.len())
                .expect("size change metrics should be valid");

            OptimizeCase {
                label,
                input_bytes,
                metrics,
            }
        })
        .collect()
}

support::slimg_criterion_group!(benches, bench_pipeline);
criterion_main!(benches);
