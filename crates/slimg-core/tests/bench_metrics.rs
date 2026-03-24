#[path = "../benches/support/mod.rs"]
mod support;

use support::{compression_metrics, size_change_metrics, MetricsError};

use slimg_core::ImageData;

fn test_image(width: u32, height: u32) -> ImageData {
    ImageData::new(width, height, vec![255; (width * height * 4) as usize])
}

#[test]
fn compression_metrics_compute_expected_values() {
    let image = test_image(10, 10);
    let metrics = compression_metrics(&image, 100).expect("metrics should be valid");

    assert_eq!(metrics.raw_bytes, 400);
    assert_eq!(metrics.encoded_bytes, 100);
    assert!((metrics.bytes_per_pixel - 1.0).abs() < 1e-9);
    assert!((metrics.compression_ratio - 4.0).abs() < 1e-9);
    assert!((metrics.space_saving_pct - 75.0).abs() < 1e-9);
}

#[test]
fn compression_metrics_reject_zero_encoded_bytes() {
    let image = test_image(10, 10);
    let err = compression_metrics(&image, 0).expect_err("zero encoded bytes should fail");
    assert_eq!(err, MetricsError::ZeroEncodedBytes);
}

#[test]
fn compression_metrics_reject_zero_dimensions() {
    let image = test_image(0, 10);
    let err = compression_metrics(&image, 10).expect_err("zero pixels should fail");
    assert_eq!(err, MetricsError::ZeroPixels);
}

#[test]
fn size_change_metrics_compute_expected_values() {
    let metrics = size_change_metrics(200, 150).expect("metrics should be valid");

    assert_eq!(metrics.input_bytes, 200);
    assert_eq!(metrics.output_bytes, 150);
    assert!((metrics.output_to_input_ratio - 0.75).abs() < 1e-9);
    assert!((metrics.size_change_pct + 25.0).abs() < 1e-9);
}

#[test]
fn size_change_metrics_reject_zero_input_bytes() {
    let err = size_change_metrics(0, 100).expect_err("zero input bytes should fail");
    assert_eq!(err, MetricsError::ZeroInputBytes);
}
