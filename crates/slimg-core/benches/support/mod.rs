#![allow(dead_code)]

use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

use serde::Serialize;
use slimg_core::ImageData;

pub const BENCH_IMAGE_SIZE: u32 = 512;
pub const BENCH_QUALITY: u8 = 80;
pub const FIXTURE_NAME: &str = "512x512 gradient RGBA";

#[derive(Debug, Clone, Serialize)]
pub struct FixtureInfo {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub raw_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CompressionMetrics {
    pub raw_bytes: u64,
    pub encoded_bytes: u64,
    pub bytes_per_pixel: f64,
    pub compression_ratio: f64,
    pub space_saving_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SizeChangeMetrics {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub output_to_input_ratio: f64,
    pub size_change_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompressionRow {
    pub label: String,
    #[serde(flatten)]
    pub metrics: CompressionMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct SizeChangeRow {
    pub label: String,
    #[serde(flatten)]
    pub metrics: SizeChangeMetrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsError {
    ZeroPixels,
    ZeroEncodedBytes,
    ZeroInputBytes,
    ArithmeticOverflow,
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPixels => write!(f, "pixel count must be non-zero"),
            Self::ZeroEncodedBytes => write!(f, "encoded byte length must be non-zero"),
            Self::ZeroInputBytes => write!(f, "input byte length must be non-zero"),
            Self::ArithmeticOverflow => write!(f, "metric calculation overflowed"),
        }
    }
}

impl std::error::Error for MetricsError {}

pub fn benchmark_fixture() -> ImageData {
    generate_test_image(BENCH_IMAGE_SIZE, BENCH_IMAGE_SIZE)
}

pub fn fixture_info(image: &ImageData) -> Result<FixtureInfo, MetricsError> {
    Ok(FixtureInfo {
        name: FIXTURE_NAME,
        width: image.width,
        height: image.height,
        raw_bytes: raw_byte_len(image.width, image.height)?,
    })
}

pub fn compression_metrics(image: &ImageData, encoded_bytes: usize) -> Result<CompressionMetrics, MetricsError> {
    let pixel_count = pixel_count(image.width, image.height)?;
    let raw_bytes = raw_byte_len(image.width, image.height)?;
    let encoded_bytes = u64::try_from(encoded_bytes).map_err(|_| MetricsError::ArithmeticOverflow)?;
    if encoded_bytes == 0 {
        return Err(MetricsError::ZeroEncodedBytes);
    }

    Ok(CompressionMetrics {
        raw_bytes,
        encoded_bytes,
        bytes_per_pixel: encoded_bytes as f64 / pixel_count as f64,
        compression_ratio: raw_bytes as f64 / encoded_bytes as f64,
        space_saving_pct: 100.0 * (1.0 - encoded_bytes as f64 / raw_bytes as f64),
    })
}

pub fn size_change_metrics(input_bytes: usize, output_bytes: usize) -> Result<SizeChangeMetrics, MetricsError> {
    let input_bytes = u64::try_from(input_bytes).map_err(|_| MetricsError::ArithmeticOverflow)?;
    let output_bytes = u64::try_from(output_bytes).map_err(|_| MetricsError::ArithmeticOverflow)?;
    if input_bytes == 0 {
        return Err(MetricsError::ZeroInputBytes);
    }

    Ok(SizeChangeMetrics {
        input_bytes,
        output_bytes,
        output_to_input_ratio: output_bytes as f64 / input_bytes as f64,
        size_change_pct: 100.0 * (output_bytes as f64 / input_bytes as f64 - 1.0),
    })
}

pub fn print_compression_table(title: &str, fixture: &FixtureInfo, quality: u8, rows: &[CompressionRow]) {
    println!();
    println!("{title}");
    println!(
        "Fixture: {} ({}x{}, raw {} bytes, quality {})",
        fixture.name, fixture.width, fixture.height, fixture.raw_bytes, quality
    );
    print_header("Format", &["Encoded bytes", "BPP", "Ratio", "Saving"]);

    for row in rows {
        println!(
            "{:<18} {:>14} {:>8.4} {:>9.2}x {:>8.2}%",
            row.label,
            row.metrics.encoded_bytes,
            row.metrics.bytes_per_pixel,
            row.metrics.compression_ratio,
            row.metrics.space_saving_pct,
        );
    }
}

pub fn print_size_change_table(title: &str, fixture: &FixtureInfo, rows: &[SizeChangeRow]) {
    println!();
    println!("{title}");
    println!(
        "Fixture: {} ({}x{}, raw {} bytes)",
        fixture.name, fixture.width, fixture.height, fixture.raw_bytes
    );
    print_header("Case", &["Input bytes", "Output bytes", "Out/In", "Change"]);

    for row in rows {
        println!(
            "{:<22} {:>12} {:>13} {:>8.4} {:>8.2}%",
            row.label,
            row.metrics.input_bytes,
            row.metrics.output_bytes,
            row.metrics.output_to_input_ratio,
            row.metrics.size_change_pct,
        );
    }
}

pub fn write_json_report<T: Serialize>(name: &str, report: &T) -> io::Result<PathBuf> {
    let path = metrics_dir().join(format!("{name}.json"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = File::create(&path)?;
    serde_json::to_writer_pretty(&mut file, report)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    file.write_all(b"\n")?;
    Ok(path)
}

fn generate_test_image(width: u32, height: u32) -> ImageData {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            data[i] = (x * 255 / width) as u8;
            data[i + 1] = (y * 255 / height) as u8;
            data[i + 2] = 128;
            data[i + 3] = 255;
        }
    }
    ImageData::new(width, height, data)
}

fn pixel_count(width: u32, height: u32) -> Result<u64, MetricsError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(MetricsError::ArithmeticOverflow)?;
    if pixels == 0 {
        return Err(MetricsError::ZeroPixels);
    }
    Ok(pixels)
}

fn raw_byte_len(width: u32, height: u32) -> Result<u64, MetricsError> {
    pixel_count(width, height)?
        .checked_mul(4)
        .ok_or(MetricsError::ArithmeticOverflow)
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

fn print_header(first_column: &str, columns: &[&str]) {
    println!(
        "{:<22} {:>12} {:>13} {:>8} {:>8}",
        first_column, columns[0], columns[1], columns[2], columns[3]
    );
    println!("{}", "-".repeat(70));
}
