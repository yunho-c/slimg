use std::fs;
use std::path::Path;

use assert_cmd::Command;
use slimg_core::{Format, ImageData, PipelineOptions, convert};
use tempfile::TempDir;

fn write_test_png(path: &Path) {
    let image = ImageData::new(8, 8, vec![120u8; 8 * 8 * 4]);
    let options = PipelineOptions {
        format: Format::Png,
        quality: 80,
        effort: None,
        png_palette: Default::default(),
        threads: None,
        resize: None,
        crop: None,
        extend: None,
        fill_color: None,
    };
    let result = convert(&image, &options).expect("encode test png");
    fs::write(path, &result.data).expect("write test png");
}

fn slimg() -> Command {
    Command::cargo_bin("slimg").expect("binary builds")
}

#[test]
fn convert_writes_output_file() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("photo.png");
    write_test_png(&input);

    slimg()
        .args(["convert", input.to_str().unwrap(), "--format", "webp"])
        .assert()
        .success();

    assert!(dir.path().join("photo.webp").exists());
}

#[test]
fn convert_refuses_existing_output_without_overwrite() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("photo.png");
    write_test_png(&input);

    slimg()
        .args(["convert", input.to_str().unwrap(), "--format", "webp"])
        .assert()
        .success();

    // Second run targets the same photo.webp and must fail without
    // --overwrite.
    slimg()
        .args(["convert", input.to_str().unwrap(), "--format", "webp"])
        .assert()
        .failure();

    slimg()
        .args([
            "convert",
            input.to_str().unwrap(),
            "--format",
            "webp",
            "--overwrite",
        ])
        .assert()
        .success();
}

#[test]
fn optimize_requires_overwrite_or_output() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("photo.png");
    write_test_png(&input);

    slimg()
        .args(["optimize", input.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--overwrite"));

    slimg()
        .args(["optimize", input.to_str().unwrap(), "--overwrite"])
        .assert()
        .success();
}

#[test]
fn batch_convert_with_output_creates_directory() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    write_test_png(&src.join("a.png"));
    write_test_png(&src.join("b.png"));
    let out = dir.path().join("out");

    slimg()
        .args([
            "convert",
            src.to_str().unwrap(),
            "--format",
            "webp",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.join("a.webp").exists());
    assert!(out.join("b.webp").exists());
}

#[test]
fn batch_convert_rejects_single_file_output() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    write_test_png(&src.join("a.png"));
    write_test_png(&src.join("b.png"));
    let out = dir.path().join("collide.webp");
    fs::write(&out, b"existing").unwrap();

    slimg()
        .args([
            "convert",
            src.to_str().unwrap(),
            "--format",
            "webp",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure();

    assert_eq!(
        fs::read(&out).unwrap(),
        b"existing",
        "colliding output file must be left untouched"
    );
}

#[test]
fn resize_supports_directory_input() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    write_test_png(&src.join("a.png"));
    write_test_png(&src.join("b.png"));
    let out = dir.path().join("out");

    slimg()
        .args([
            "resize",
            src.to_str().unwrap(),
            "--width",
            "4",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.join("a.png").exists());
    assert!(out.join("b.png").exists());
}
