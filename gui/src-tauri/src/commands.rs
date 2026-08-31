use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use slimg_core::{
    codec::get_codec, CropMode, EncodeOptions, ExtendMode, FillColor, Format, ImageData,
    PipelineOptions, ResizeMode,
};
use tauri::Emitter;

/// Shared cancellation flag for the currently running batch.
#[derive(Default)]
pub struct BatchCancel(AtomicBool);

// ── Constants ─────────────────────────────────────────────────

const THUMBNAIL_MAX_DIMENSION: u32 = 400;
const THUMBNAIL_PNG_COMPRESSION: u8 = 90;
const MAX_UNIQUE_ATTEMPTS: u32 = 9999;

// ── Types ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub size_bytes: u64,
    pub thumbnail_base64: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Convert,
    Optimize,
    Resize,
    Crop,
    Extend,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessOptions {
    pub operation: Operation,
    pub format: Option<String>,
    pub quality: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub x: Option<u32>,
    pub y: Option<u32>,
    pub crop_mode: Option<String>,
    pub fill_color: Option<String>,
    pub resize_mode: Option<String>,
    pub output_dir: Option<String>,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessResult {
    pub output_path: String,
    pub original_size: u64,
    pub new_size: u64,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Serialize)]
pub struct PreviewResult {
    pub data_base64: String,
    pub size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchProgress {
    pub index: usize,
    pub total: usize,
    pub file_path: String,
    pub status: String,
    pub result: Option<ProcessResult>,
    pub error: Option<String>,
}

// ── Commands ───────────────────────────────────────────────────

#[tauri::command]
pub async fn scan_directory(path: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir_path = Path::new(&path);
        if !dir_path.is_dir() {
            return Err(format!("Not a directory: {}", path));
        }

        let mut files = Vec::new();
        collect_images(dir_path, &mut files)?;
        files.sort();
        Ok(files)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

fn collect_images(dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_images(&path, out)?;
        } else if file_type.is_file() && Format::from_extension(&path).is_some() {
            out.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn load_image(path: String) -> Result<ImageInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let file_path = Path::new(&path);

        if Format::from_extension(file_path).is_none() {
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            return Err(format!("Unsupported file type: {}", ext));
        }

        let raw_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let size_bytes = raw_bytes.len() as u64;

        let (image, format) = slimg_core::decode(&raw_bytes).map_err(|e| e.to_string())?;

        let thumbnail = slimg_core::resize::resize(
            &image,
            &ResizeMode::Fit(THUMBNAIL_MAX_DIMENSION, THUMBNAIL_MAX_DIMENSION),
        )
        .map_err(|e| e.to_string())?;

        let png_bytes = encode_as_png(&thumbnail)?;
        let thumbnail_base64 = BASE64.encode(&png_bytes);

        Ok(ImageInfo {
            width: image.width,
            height: image.height,
            format: format.extension().to_string(),
            size_bytes,
            thumbnail_base64,
        })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn process_image(
    input: String,
    options: ProcessOptions,
) -> Result<ProcessResult, String> {
    tauri::async_runtime::spawn_blocking(move || process_single_file(&input, &options))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn preview_image(
    input: String,
    options: ProcessOptions,
) -> Result<PreviewResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let input_path = Path::new(&input);

        let raw_bytes = std::fs::read(input_path).map_err(|e| e.to_string())?;
        let (image, source_format) = slimg_core::decode(&raw_bytes).map_err(|e| e.to_string())?;

        let pipeline_result = if matches!(options.operation, Operation::Optimize) {
            slimg_core::optimize(&raw_bytes, options.quality).map_err(|e| e.to_string())?
        } else {
            let pipeline_options = build_pipeline_options(&options, source_format)?;
            slimg_core::convert(&image, &pipeline_options).map_err(|e| e.to_string())?
        };

        let data_base64 = BASE64.encode(&pipeline_result.data);

        Ok(PreviewResult {
            data_base64,
            size_bytes: pipeline_result.data.len() as u64,
            width: pipeline_result.width,
            height: pipeline_result.height,
            format: pipeline_result.format.extension().to_string(),
        })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn process_batch(
    inputs: Vec<String>,
    options: ProcessOptions,
    window: tauri::Window,
    cancel: tauri::State<'_, BatchCancel>,
) -> Result<(), String> {
    let total = inputs.len();
    cancel.0.store(false, Ordering::SeqCst);

    for (index, file_path) in inputs.iter().enumerate() {
        if cancel.0.load(Ordering::SeqCst) {
            // Mark every unprocessed file as cancelled and stop.
            for (rest_index, rest_path) in inputs.iter().enumerate().skip(index) {
                emit_progress(
                    &window,
                    BatchProgress {
                        index: rest_index,
                        total,
                        file_path: rest_path.clone(),
                        status: "cancelled".to_string(),
                        result: None,
                        error: None,
                    },
                );
            }
            return Ok(());
        }

        emit_progress(
            &window,
            BatchProgress {
                index,
                total,
                file_path: file_path.clone(),
                status: "processing".to_string(),
                result: None,
                error: None,
            },
        );

        let fp = file_path.clone();
        let opts = options.clone();
        // A panic inside the blocking task must fail this file only, not
        // abort the remaining batch.
        let result = tauri::async_runtime::spawn_blocking(move || process_single_file(&fp, &opts))
            .await
            .unwrap_or_else(|e| Err(format!("{}: task panicked: {}", file_path, e)));

        let progress = match result {
            Ok(result) => BatchProgress {
                index,
                total,
                file_path: file_path.clone(),
                status: "completed".to_string(),
                result: Some(result),
                error: None,
            },
            Err(err) => BatchProgress {
                index,
                total,
                file_path: file_path.clone(),
                status: "error".to_string(),
                result: None,
                error: Some(err),
            },
        };
        emit_progress(&window, progress);
    }

    Ok(())
}

#[tauri::command]
pub fn cancel_batch(cancel: tauri::State<'_, BatchCancel>) {
    cancel.0.store(true, Ordering::SeqCst);
}

fn emit_progress(window: &tauri::Window, progress: BatchProgress) {
    if let Err(e) = window.emit("batch-progress", &progress) {
        eprintln!("failed to emit batch progress: {e}");
    }
}

// ── Helpers ────────────────────────────────────────────────────

fn process_single_file(input: &str, options: &ProcessOptions) -> Result<ProcessResult, String> {
    let input_path = Path::new(input);

    let raw_bytes = std::fs::read(input_path).map_err(|e| format!("{}: {}", input, e))?;
    let original_size = raw_bytes.len() as u64;

    let (image, source_format) =
        slimg_core::decode(&raw_bytes).map_err(|e| format!("{}: {}", input, e))?;

    let pipeline_result = if matches!(options.operation, Operation::Optimize) {
        slimg_core::optimize(&raw_bytes, options.quality)
            .map_err(|e| format!("{}: {}", input, e))?
    } else {
        let pipeline_options = build_pipeline_options(options, source_format)?;
        slimg_core::convert(&image, &pipeline_options).map_err(|e| format!("{}: {}", input, e))?
    };

    let output_dir = options.output_dir.as_deref().map(Path::new);
    if let Some(dir) = output_dir {
        // output_path treats a non-directory target as a single file path,
        // which would collapse every batch output onto it. Make sure the
        // configured output directory actually exists as a directory.
        if dir.exists() && !dir.is_dir() {
            return Err(format!(
                "output directory is not a directory: {}",
                dir.display()
            ));
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    }
    let mut out_path = slimg_core::output_path(input_path, pipeline_result.format, output_dir);

    if !options.overwrite && out_path.exists() {
        out_path = find_unique_path(&out_path)?;
    }

    pipeline_result
        .save(&out_path)
        .map_err(|e| format!("{}: {}", out_path.display(), e))?;

    Ok(ProcessResult {
        output_path: out_path.to_string_lossy().to_string(),
        original_size,
        new_size: pipeline_result.data.len() as u64,
        width: pipeline_result.width,
        height: pipeline_result.height,
        format: pipeline_result.format.extension().to_string(),
    })
}

fn build_pipeline_options(
    options: &ProcessOptions,
    source_format: Format,
) -> Result<PipelineOptions, String> {
    let format = match &options.format {
        Some(f) => parse_format(f)?,
        None => source_format,
    };

    let resize = match options.operation {
        Operation::Resize => Some(match options.resize_mode.as_deref().unwrap_or("width") {
            "width" => ResizeMode::Width(require(options.width, "width")?),
            "height" => ResizeMode::Height(require(options.height, "height")?),
            "exact" => ResizeMode::Exact(
                require(options.width, "width")?,
                require(options.height, "height")?,
            ),
            "fit" => ResizeMode::Fit(
                require(options.width, "width")?,
                require(options.height, "height")?,
            ),
            other => return Err(format!("Unknown resize mode: {}", other)),
        }),
        _ => None,
    };

    let crop = match options.operation {
        Operation::Crop => Some(match options.crop_mode.as_deref().unwrap_or("aspect") {
            "region" => CropMode::Region {
                x: options.x.unwrap_or(0),
                y: options.y.unwrap_or(0),
                width: require(options.width, "width")?,
                height: require(options.height, "height")?,
            },
            "aspect" => CropMode::AspectRatio {
                width: require(options.width, "aspect width")?,
                height: require(options.height, "aspect height")?,
            },
            other => return Err(format!("Unknown crop mode: {}", other)),
        }),
        _ => None,
    };

    let extend = match options.operation {
        Operation::Extend => Some(ExtendMode::AspectRatio {
            width: require(options.width, "aspect width")?,
            height: require(options.height, "aspect height")?,
        }),
        _ => None,
    };

    let fill_color = options
        .fill_color
        .as_ref()
        .map(|hex| parse_hex_color(hex))
        .transpose()?;

    Ok(PipelineOptions {
        format,
        quality: options.quality,
        effort: None,
        png_palette: Default::default(),
        threads: None,
        resize,
        crop,
        extend,
        fill_color,
    })
}

fn require(value: Option<u32>, name: &str) -> Result<u32, String> {
    match value {
        Some(v) if v > 0 => Ok(v),
        _ => Err(format!("{} is required", name)),
    }
}

fn parse_format(s: &str) -> Result<Format, String> {
    match s.to_lowercase().as_str() {
        "jpeg" | "jpg" => Ok(Format::Jpeg),
        "png" => Ok(Format::Png),
        "webp" => Ok(Format::WebP),
        "avif" => Ok(Format::Avif),
        "jxl" => Ok(Format::Jxl),
        "qoi" => Ok(Format::Qoi),
        _ => Err(format!("Unknown format: {}", s)),
    }
}

fn parse_hex_color(hex: &str) -> Result<FillColor, String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 && hex.len() != 8 {
        return Err("Invalid hex color".to_string());
    }
    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?;
    let a = if hex.len() == 8 {
        u8::from_str_radix(&hex[6..8], 16).map_err(|e| e.to_string())?
    } else {
        255
    };
    Ok(FillColor::Solid([r, g, b, a]))
}

fn encode_as_png(image: &ImageData) -> Result<Vec<u8>, String> {
    let codec = get_codec(Format::Png);
    let opts = EncodeOptions {
        quality: THUMBNAIL_PNG_COMPRESSION,
        effort: None,
        png_palette: Default::default(),
        threads: None,
    };
    codec.encode(image, &opts).map_err(|e| e.to_string())
}

fn find_unique_path(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }

    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let parent = path.parent().unwrap_or(Path::new("."));

    for counter in 1..=MAX_UNIQUE_ATTEMPTS {
        let candidate = parent.join(format!("{}_{}.{}", stem, counter, ext));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "Could not find unique path after {} attempts for: {}",
        MAX_UNIQUE_ATTEMPTS,
        path.display()
    ))
}
