use std::env;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use plotters::prelude::*;
use serde::Serialize;
use slimg_core::codec::jxl::{
    JxlEncodeBackend, JxlEncodeOutcome, JxlFallbackReason, encode_with_diagnostics,
};
use slimg_core::codec::{EncodeOptions, get_codec};
use slimg_core::resize::{ResizeMode, resize};
use slimg_core::{Codec, Format, ImageData, JxlEncoderPreference, decode_file};

const NATURAL_BENCH_ENV: &str = "SLIMG_BENCH_NATURAL_DIR";
const NATURAL_BENCH_DEFAULT_REPO: &str =
    "GitHub/Kodak-Lossless-True-Color-Image-Suite/PhotoCD_PCD0992";
const SWEEP_DIMS_ENV: &str = "SLIMG_SWEEP_MAX_DIMS";
const SWEEP_QUALITY_ENV: &str = "SLIMG_SWEEP_QUALITY";
const SWEEP_REPEATS_ENV: &str = "SLIMG_SWEEP_REPEATS";
const DEFAULT_QUALITY: u8 = 80;
const DEFAULT_REPEATS: usize = 3;
const DEFAULT_DIMS: &[u32] = &[128, 256, 384, 512, 640, 768];

#[derive(Debug, Clone)]
struct NaturalImage {
    image: ImageData,
}

#[derive(Debug, Clone, Serialize)]
struct NaturalCorpusInfo {
    name: String,
    image_count: usize,
    source_dir: String,
    original_width: u32,
    original_height: u32,
}

#[derive(Debug, Clone)]
struct NaturalCorpus {
    images: Vec<NaturalImage>,
    info: NaturalCorpusInfo,
}

#[derive(Debug, Clone)]
struct ResizedCorpus {
    max_dim: u32,
    width: u32,
    height: u32,
    total_pixels: u64,
    images: Vec<ImageData>,
}

#[derive(Debug, Clone, Serialize)]
struct SweepPoint {
    max_dim: u32,
    width: u32,
    height: u32,
    total_pixels: u64,
    total_encoded_bytes: u64,
    median_duration_ms: f64,
    ms_per_image: f64,
    throughput_mpx_s: f64,
}

#[derive(Debug, Clone, Serialize)]
struct CodecSeries {
    label: String,
    points: Vec<SweepPoint>,
}

#[derive(Debug, Clone, Serialize)]
struct SweepReport {
    corpus: NaturalCorpusInfo,
    quality: u8,
    repeats: usize,
    max_dims: Vec<u32>,
    series: Vec<CodecSeries>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(corpus) = load_natural_corpus() else {
        return Ok(());
    };

    let quality = env::var(SWEEP_QUALITY_ENV)
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(DEFAULT_QUALITY);
    let repeats = env::var(SWEEP_REPEATS_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_REPEATS);
    let max_dims = sweep_dims(&corpus);
    let resized = max_dims
        .iter()
        .copied()
        .map(|max_dim| resize_corpus(&corpus, max_dim))
        .collect::<Result<Vec<_>, _>>()?;

    let series = encodable_formats()
        .into_iter()
        .map(|format| measure_format(format, quality, repeats, &resized))
        .collect::<Result<Vec<_>, _>>()?;

    let report = SweepReport {
        corpus: corpus.info.clone(),
        quality,
        repeats,
        max_dims,
        series,
    };

    let json_path = write_json_report("natural_resolution_sweep", &report)?;
    let svg_path = metrics_dir().join("natural_resolution_sweep.svg");
    write_svg_chart(&svg_path, &report)?;
    print_summary(&report, &svg_path);

    println!();
    println!("Wrote JSON: {}", json_path.display());
    println!("Wrote SVG:  {}", svg_path.display());

    Ok(())
}

fn load_natural_corpus() -> Option<NaturalCorpus> {
    let source_dir = match natural_bench_dir() {
        Some(path) => path,
        None => {
            eprintln!(
                "Skipping natural resolution sweep: set {NATURAL_BENCH_ENV} or clone the Kodak corpus under ~/{}",
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
                "Skipping natural resolution sweep: failed to read {}: {err}",
                source_dir.display(),
            );
            return None;
        }
    };
    paths.sort();

    if paths.is_empty() {
        eprintln!(
            "Skipping natural resolution sweep: no PNG images found in {}",
            source_dir.display(),
        );
        return None;
    }

    let images = paths
        .iter()
        .map(|path| {
            let (image, format) = decode_file(path).unwrap_or_else(|err| {
                panic!(
                    "failed to decode natural resolution sweep image {}: {err}",
                    path.display()
                )
            });
            assert_eq!(
                format,
                Format::Png,
                "natural resolution sweep corpus must contain PNG files"
            );
            NaturalImage { image }
        })
        .collect::<Vec<_>>();

    let original_width = images[0].image.width;
    let original_height = images[0].image.height;

    Some(NaturalCorpus {
        images,
        info: NaturalCorpusInfo {
            name: "Kodak Lossless True Color Image Suite".to_string(),
            image_count: paths.len(),
            source_dir: source_dir.display().to_string(),
            original_width,
            original_height,
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

fn sweep_dims(corpus: &NaturalCorpus) -> Vec<u32> {
    let source_max_dim = corpus.info.original_width.max(corpus.info.original_height);

    let mut dims = env::var(SWEEP_DIMS_ENV)
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .filter(|dim| *dim > 0)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| DEFAULT_DIMS.to_vec());

    dims.retain(|dim| *dim <= source_max_dim);
    dims.sort_unstable();
    dims.dedup();

    if dims.is_empty() {
        vec![source_max_dim]
    } else {
        dims
    }
}

fn resize_corpus(
    corpus: &NaturalCorpus,
    max_dim: u32,
) -> Result<ResizedCorpus, Box<dyn std::error::Error>> {
    let images = corpus
        .images
        .iter()
        .map(|image| {
            if image.image.width.max(image.image.height) == max_dim {
                Ok(image.image.clone())
            } else {
                resize(&image.image, &ResizeMode::Fit(max_dim, max_dim))
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let width = images[0].width;
    let height = images[0].height;
    let total_pixels = images
        .iter()
        .map(|image| u64::from(image.width) * u64::from(image.height))
        .sum::<u64>();

    Ok(ResizedCorpus {
        max_dim,
        width,
        height,
        total_pixels,
        images,
    })
}

fn measure_format(
    format: Format,
    quality: u8,
    repeats: usize,
    corpora: &[ResizedCorpus],
) -> Result<CodecSeries, Box<dyn std::error::Error>> {
    let codec = get_codec(format);
    let mut jxl_diagnostics = None;
    let mut points = Vec::with_capacity(corpora.len());

    for corpus in corpora {
        let options = EncodeOptions {
            quality,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: JxlEncoderPreference::PreferGjxl,
            threads: None,
        };

        for image in &corpus.images {
            let encoded = encode_for_sweep(
                codec.as_ref(),
                format,
                image,
                &options,
                &mut jxl_diagnostics,
            )?;
            black_box(encoded.len());
        }

        let mut durations = Vec::with_capacity(repeats);
        let mut encoded_bytes = 0u64;

        for repeat in 0..repeats {
            let start = Instant::now();
            let mut total_encoded_bytes = 0u64;
            for image in &corpus.images {
                let encoded = encode_for_sweep(
                    codec.as_ref(),
                    format,
                    image,
                    &options,
                    &mut jxl_diagnostics,
                )?;
                total_encoded_bytes += encoded.len() as u64;
                black_box(encoded);
            }
            let elapsed = start.elapsed();
            if repeat == 0 {
                encoded_bytes = total_encoded_bytes;
            }
            durations.push(elapsed);
        }

        let median = median_duration(&mut durations);
        let secs = median.as_secs_f64();
        let throughput_mpx_s = if secs > 0.0 {
            corpus.total_pixels as f64 / 1_000_000.0 / secs
        } else {
            0.0
        };
        let ms_per_image = median.as_secs_f64() * 1_000.0 / corpus.images.len() as f64;

        points.push(SweepPoint {
            max_dim: corpus.max_dim,
            width: corpus.width,
            height: corpus.height,
            total_pixels: corpus.total_pixels,
            total_encoded_bytes: encoded_bytes,
            median_duration_ms: median.as_secs_f64() * 1_000.0,
            ms_per_image,
            throughput_mpx_s,
        });
    }

    let label = jxl_diagnostics
        .as_ref()
        .map(jxl_diagnostic_label)
        .unwrap_or_else(|| format_label(format).to_string());
    Ok(CodecSeries { label, points })
}

type JxlDiagnostic = (JxlEncodeBackend, Option<JxlFallbackReason>);

fn encode_for_sweep(
    codec: &dyn Codec,
    format: Format,
    image: &ImageData,
    options: &EncodeOptions,
    expected: &mut Option<JxlDiagnostic>,
) -> slimg_core::Result<Vec<u8>> {
    if format != Format::Jxl {
        return codec.encode(image, options);
    }

    let outcome = encode_with_diagnostics(image, options)?;
    assert_consistent_jxl_backend(expected, &outcome);
    Ok(outcome.data)
}

fn assert_consistent_jxl_backend(expected: &mut Option<JxlDiagnostic>, outcome: &JxlEncodeOutcome) {
    let observed = (outcome.backend, outcome.fallback_reason.clone());
    if let Some(expected) = expected {
        assert_eq!(
            *expected, observed,
            "JXL sweep mixed encoder backends or fallback reasons"
        );
    } else {
        *expected = Some(observed);
    }
}

fn jxl_diagnostic_label(diagnostic: &JxlDiagnostic) -> String {
    match diagnostic {
        (JxlEncodeBackend::Gjxl, None) => "JXL/GJXL".into(),
        (backend, reason) => format!("JXL/{backend:?}/{reason:?}"),
    }
}

fn median_duration(durations: &mut [Duration]) -> Duration {
    durations.sort_unstable();
    durations[durations.len() / 2]
}

fn print_summary(report: &SweepReport, svg_path: &Path) {
    println!();
    println!("Natural resolution sweep");
    println!(
        "Corpus: {} ({} images, source {}x{})",
        report.corpus.name,
        report.corpus.image_count,
        report.corpus.original_width,
        report.corpus.original_height,
    );
    println!("Source dir: {}", report.corpus.source_dir);
    println!(
        "Quality: {}, repeats per point: {}, max dims: {:?}",
        report.quality, report.repeats, report.max_dims
    );
    println!("Chart: {}", svg_path.display());

    for series in &report.series {
        println!();
        println!(
            "{:<8} {:>8} {:>12} {:>12}",
            "Codec", "MaxDim", "MPx/s", "ms/image"
        );
        println!("{}", "-".repeat(48));
        for point in &series.points {
            println!(
                "{:<8} {:>8} {:>12.2} {:>12.2}",
                series.label, point.max_dim, point.throughput_mpx_s, point.ms_per_image
            );
        }
    }
}

fn write_svg_chart(path: &Path, report: &SweepReport) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let x_min = report.max_dims.iter().copied().min().unwrap_or(1) as f64;
    let x_max = report.max_dims.iter().copied().max().unwrap_or(1) as f64;
    let y_values = report
        .series
        .iter()
        .flat_map(|series| series.points.iter().map(|point| point.throughput_mpx_s))
        .filter(|value| *value > 0.0)
        .collect::<Vec<_>>();
    let min_y = y_values.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = y_values.iter().copied().fold(0.0_f64, f64::max);
    let min_y = if min_y.is_finite() {
        min_y * 0.85
    } else {
        1e-3
    };
    let max_y = if max_y > 0.0 { max_y * 1.15 } else { 1.0 };

    let root = SVGBackend::new(path, (1280, 720)).into_drawing_area();
    root.fill(&WHITE).map_err(plotters_err)?;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Natural Encode Sweep by Resolution ({}, quality {})",
                report.corpus.name, report.quality
            ),
            ("sans-serif", 30),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(70)
        .build_cartesian_2d((x_min..x_max).log_scale(), (min_y..max_y).log_scale())
        .map_err(plotters_err)?;

    chart
        .configure_mesh()
        .x_desc("Max dimension (log scale)")
        .y_desc("Encode throughput (MPx/s, log scale)")
        .draw()
        .map_err(plotters_err)?;

    for (index, series) in report.series.iter().enumerate() {
        let line_color = Palette99::pick(index);
        let point_color = Palette99::pick(index);
        let legend_color = Palette99::pick(index);
        let line_points = series
            .points
            .iter()
            .filter(|point| point.throughput_mpx_s > 0.0)
            .map(|point| (point.max_dim as f64, point.throughput_mpx_s))
            .collect::<Vec<_>>();

        chart
            .draw_series(LineSeries::new(line_points.clone(), &line_color))
            .map_err(plotters_err)?
            .label(series.label.clone())
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &legend_color));

        chart
            .draw_series(
                line_points
                    .into_iter()
                    .map(|point| Circle::new(point, 4, point_color.filled())),
            )
            .map_err(plotters_err)?;
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.85))
        .border_style(BLACK)
        .draw()
        .map_err(plotters_err)?;

    root.present().map_err(plotters_err)?;
    Ok(())
}

fn plotters_err<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::other(err.to_string())
}

fn write_json_report<T: Serialize>(name: &str, report: &T) -> io::Result<PathBuf> {
    let path = metrics_dir().join(format!("{name}.json"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = File::create(&path)?;
    serde_json::to_writer_pretty(&mut file, report).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    Ok(path)
}

fn metrics_dir() -> PathBuf {
    target_dir().join("criterion").join("slimg-metrics")
}

fn target_dir() -> PathBuf {
    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .unwrap_or(&manifest_dir)
        .join("target")
}

fn encodable_formats() -> Vec<Format> {
    vec![
        Format::Jpeg,
        Format::Jxl,
        Format::WebP,
        Format::Avif,
        Format::Png,
        Format::Qoi,
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
