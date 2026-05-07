use std::io::Cursor;

use image::ImageEncoder;
use image::codecs::png::PngEncoder;

use crate::error::{Error, Result};
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};

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

        let raw_bytes = raw_png.into_inner();

        let preset = options
            .effort
            .map(effort_to_oxipng_preset)
            .unwrap_or_else(|| quality_to_oxipng_preset(options.quality));

        let opts = oxipng::Options::from_preset(preset);
        let optimized = oxipng::optimize_from_memory(&raw_bytes, &opts)
            .map_err(|e| Error::Encode(format!("oxipng optimize: {e}")))?;

        Ok(optimized)
    }
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
}
