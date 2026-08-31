use std::collections::HashMap;
use std::io::Cursor;

use image::ImageEncoder;
use image::codecs::png::PngEncoder;

use crate::error::{Error, Result};
use crate::format::Format;
use crate::palette::{PaletteRecommendation, analyze_palette_suitability};

use super::{Codec, EncodeOptions, ImageData, PngPaletteMode};

/// PNG codec backed by OxiPNG for optimization.
pub struct PngCodec;

impl Codec for PngCodec {
    fn format(&self) -> Format {
        Format::Png
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Png)
            .map_err(|e| Error::Decode(format!("png decode: {e}")))?;

        let rgba = img.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        Ok(ImageData::new(width, height, rgba.into_raw()))
    }

    fn encode(&self, image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
        match options.png_palette {
            PngPaletteMode::Off => encode_rgba_png(image, options),
            PngPaletteMode::On => encode_palette_png(image, options).map(|output| output.data),
            PngPaletteMode::Auto => {
                let rgba = encode_rgba_png(image, options)?;
                if !palette_candidate_allowed(image) {
                    return Ok(rgba);
                }

                let palette = match encode_palette_png(image, options) {
                    Ok(output) => output,
                    Err(_) => return Ok(rgba),
                };

                if should_use_auto_palette(&rgba, &palette) {
                    Ok(palette.data)
                } else {
                    Ok(rgba)
                }
            }
        }
    }
}

struct PalettePngOutput {
    data: Vec<u8>,
    lossless: bool,
    remapping_quality: Option<u8>,
}

fn encode_rgba_png(image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
    // First, encode as raw PNG using the image crate's PngEncoder.
    let mut raw_png = Cursor::new(Vec::new());
    PngEncoder::new(&mut raw_png)
        .write_image(
            &image.data,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| Error::Encode(format!("png raw encode: {e}")))?;

    optimize_png(raw_png.into_inner(), options)
}

fn optimize_png(raw_bytes: Vec<u8>, options: &EncodeOptions) -> Result<Vec<u8>> {
    let preset = options
        .effort
        .map(effort_to_oxipng_preset)
        .unwrap_or_else(|| quality_to_oxipng_preset(options.quality));

    let opts = oxipng::Options::from_preset(preset);
    oxipng::optimize_from_memory(&raw_bytes, &opts)
        .map_err(|e| Error::Encode(format!("oxipng optimize: {e}")))
}

fn palette_candidate_allowed(image: &ImageData) -> bool {
    matches!(
        analyze_palette_suitability(image).recommendation,
        PaletteRecommendation::On | PaletteRecommendation::Review
    )
}

fn should_use_auto_palette(rgba: &[u8], palette: &PalettePngOutput) -> bool {
    if palette.lossless {
        palette.data.len() < rgba.len()
    } else {
        palette
            .remapping_quality
            .is_some_and(|quality| quality >= 90)
            && palette.data.len() * 100 <= rgba.len() * 95
    }
}

fn encode_palette_png(image: &ImageData, options: &EncodeOptions) -> Result<PalettePngOutput> {
    if let Some(output) = encode_exact_palette_png(image, options)? {
        return Ok(output);
    }

    encode_quantized_palette_png(image, options)
}

fn encode_exact_palette_png(
    image: &ImageData,
    options: &EncodeOptions,
) -> Result<Option<PalettePngOutput>> {
    let mut color_to_index: HashMap<u32, u8> = HashMap::new();
    let mut palette = Vec::new();
    let mut indices = Vec::with_capacity(image.data.len() / 4);

    for pixel in image.data.chunks_exact(4) {
        let color = u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
        let index = if let Some(index) = color_to_index.get(&color) {
            *index
        } else {
            if palette.len() >= 256 {
                return Ok(None);
            }
            let index = palette.len() as u8;
            color_to_index.insert(color, index);
            palette.push([pixel[0], pixel[1], pixel[2], pixel[3]]);
            index
        };
        indices.push(index);
    }

    let raw = encode_indexed_png(image.width, image.height, &palette, &indices)?;
    Ok(Some(PalettePngOutput {
        data: optimize_png(raw, options)?,
        lossless: true,
        remapping_quality: Some(100),
    }))
}

#[cfg(feature = "pngquant")]
fn encode_quantized_palette_png(
    image: &ImageData,
    options: &EncodeOptions,
) -> Result<PalettePngOutput> {
    let pixels = image
        .data
        .chunks_exact(4)
        .map(|pixel| imagequant::RGBA {
            r: pixel[0],
            g: pixel[1],
            b: pixel[2],
            a: pixel[3],
        })
        .collect::<Vec<_>>();

    let mut attr = imagequant::new();
    attr.set_quality(85, 95)
        .map_err(|e| Error::Encode(format!("imagequant quality: {e:?}")))?;
    attr.set_speed(effort_to_imagequant_speed(options.effort))
        .map_err(|e| Error::Encode(format!("imagequant speed: {e:?}")))?;

    let mut quant_image = attr
        .new_image(pixels, image.width as usize, image.height as usize, 0.0)
        .map_err(|e| Error::Encode(format!("imagequant image: {e:?}")))?;
    let mut quantization = attr
        .quantize(&mut quant_image)
        .map_err(|e| Error::Encode(format!("imagequant quantize: {e:?}")))?;
    quantization
        .set_dithering_level(1.0)
        .map_err(|e| Error::Encode(format!("imagequant dither: {e:?}")))?;
    let (palette, indices) = quantization
        .remapped(&mut quant_image)
        .map_err(|e| Error::Encode(format!("imagequant remap: {e:?}")))?;
    let remapping_quality = quantization.remapping_quality();
    let palette = palette
        .into_iter()
        .map(|color| [color.r, color.g, color.b, color.a])
        .collect::<Vec<_>>();

    let raw = encode_indexed_png(image.width, image.height, &palette, &indices)?;
    Ok(PalettePngOutput {
        data: optimize_png(raw, options)?,
        lossless: false,
        remapping_quality,
    })
}

#[cfg(not(feature = "pngquant"))]
fn encode_quantized_palette_png(
    _image: &ImageData,
    _options: &EncodeOptions,
) -> Result<PalettePngOutput> {
    Err(Error::Encode(
        "png palette quantization requires the pngquant feature".to_string(),
    ))
}

#[cfg(feature = "pngquant")]
fn encode_indexed_png(
    width: u32,
    height: u32,
    palette: &[[u8; 4]],
    indices: &[u8],
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(
        palette
            .iter()
            .flat_map(|color| [color[0], color[1], color[2]])
            .collect::<Vec<_>>(),
    );
    if palette.iter().any(|color| color[3] < 255) {
        encoder.set_trns(palette.iter().map(|color| color[3]).collect::<Vec<_>>());
    }
    let mut writer = encoder
        .write_header()
        .map_err(|e| Error::Encode(format!("png indexed header: {e}")))?;
    writer
        .write_image_data(indices)
        .map_err(|e| Error::Encode(format!("png indexed data: {e}")))?;
    drop(writer);
    Ok(bytes)
}

#[cfg(not(feature = "pngquant"))]
fn encode_indexed_png(
    _width: u32,
    _height: u32,
    _palette: &[[u8; 4]],
    _indices: &[u8],
) -> Result<Vec<u8>> {
    Err(Error::Encode(
        "png palette encoding requires the pngquant feature".to_string(),
    ))
}

fn quality_to_oxipng_preset(quality: u8) -> u8 {
    match quality {
        90..=100 => 1,
        70..=89 => 2,
        50..=69 => 3,
        30..=49 => 4,
        _ => 6,
    }
}

fn effort_to_oxipng_preset(effort: u8) -> u8 {
    let effort = effort.min(100) as u16;
    if effort <= 50 {
        ((effort * 2 + 25) / 50) as u8
    } else {
        (2 + ((effort - 50) * 4 + 25) / 50) as u8
    }
}

#[cfg(feature = "pngquant")]
fn effort_to_imagequant_speed(effort: Option<u8>) -> i32 {
    let effort = effort.unwrap_or(50).min(100) as u16;
    (10 - ((effort * 9 + 50) / 100) as i32).clamp(1, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_image(width: u32, height: u32) -> ImageData {
        let size = (width * height * 4) as usize;
        let mut data = vec![0u8; size];
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                data[i] = (x * 255 / width) as u8; // R
                data[i + 1] = (y * 255 / height) as u8; // G
                data[i + 2] = 128; // B
                data[i + 3] = 255; // A
            }
        }
        ImageData::new(width, height, data)
    }

    #[test]
    fn encode_and_decode_roundtrip() {
        let codec = PngCodec;
        let original = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");

        // Verify PNG magic bytes
        assert!(
            encoded.len() >= 4,
            "encoded data too short: {} bytes",
            encoded.len()
        );
        assert_eq!(
            &encoded[..4],
            &[0x89, 0x50, 0x4E, 0x47],
            "missing PNG magic bytes"
        );

        // Decode back and verify lossless roundtrip
        let decoded = codec.decode(&encoded).expect("decode failed");
        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(decoded.data, original.data, "PNG should be lossless");
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let codec = PngCodec;
        let result = codec.decode(b"not a png");
        assert!(result.is_err(), "decoding invalid data should fail");
    }

    #[test]
    #[cfg(feature = "pngquant")]
    fn palette_encode_roundtrips_low_color_image() {
        let codec = PngCodec;
        let original = ImageData::new(
            2,
            2,
            vec![
                255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
            ],
        );
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            png_palette: PngPaletteMode::On,
            jxl_encoder: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.data, original.data);
    }
}
