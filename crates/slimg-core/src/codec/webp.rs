use crate::error::{Error, Result};
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};

/// WebP codec backed by libwebp.
pub struct WebPCodec;

impl Codec for WebPCodec {
    fn format(&self) -> Format {
        Format::WebP
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        let img = image::load_from_memory_with_format(data, image::ImageFormat::WebP)
            .map_err(|e| Error::Decode(format!("webp decode: {e}")))?;

        let rgba = img.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();

        Ok(ImageData::new(width, height, rgba.into_raw()))
    }

    fn encode(&self, image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
        let encoder = webp::Encoder::from_rgba(&image.data, image.width, image.height);
        let lossless = options.quality >= 100;
        let encoded = if lossless || options.effort.is_some() {
            let mut config = webp::WebPConfig::new()
                .map_err(|error| Error::Encode(format!("webp config init: {error:?}")))?;
            config.lossless = if lossless { 1 } else { 0 };
            config.exact = if lossless { 1 } else { 0 };
            config.quality = options.quality as f32;
            config.method = effort_to_webp_method(options.effort.unwrap_or(50));
            encoder
                .encode_advanced(&config)
                .map_err(|error| Error::Encode(format!("webp encode: {error:?}")))?
        } else {
            encoder.encode(options.quality as f32)
        };

        Ok(encoded.to_vec())
    }
}

fn effort_to_webp_method(effort: u8) -> i32 {
    let effort = effort.min(100) as u16;
    if effort <= 50 {
        ((effort * 4 + 25) / 50) as i32
    } else {
        (4 + ((effort - 50) * 2 + 25) / 50) as i32
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

    fn create_alpha_test_image(width: u32, height: u32) -> ImageData {
        let size = (width * height * 4) as usize;
        let mut data = vec![0u8; size];
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                data[i] = ((x * 17 + y * 3) % 256) as u8;
                data[i + 1] = ((x * 5 + y * 19) % 256) as u8;
                data[i + 2] = ((x * 11 + y * 7) % 256) as u8;
                data[i + 3] = ((x * 13 + y * 23) % 256) as u8;
            }
        }
        ImageData::new(width, height, data)
    }

    #[test]
    fn encode_and_decode_roundtrip() {
        let codec = WebPCodec;
        let original = create_alpha_test_image(64, 48);
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");

        // Decode back and verify dimensions
        let decoded = codec.decode(&encoded).expect("decode failed");
        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(
            decoded.data.len(),
            (decoded.width * decoded.height * 4) as usize
        );
    }

    #[test]
    fn lower_quality_produces_smaller_file() {
        let codec = WebPCodec;
        let image = create_test_image(128, 96);

        let high = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 95,
                    effort: None,
                    png_palette: Default::default(),
                    jxl_encoder: Default::default(),
                    threads: None,
                },
            )
            .expect("encode q95 failed");
        let low = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 20,
                    effort: None,
                    png_palette: Default::default(),
                    jxl_encoder: Default::default(),
                    threads: None,
                },
            )
            .expect("encode q20 failed");

        assert!(
            low.len() < high.len(),
            "low quality ({} bytes) should be smaller than high quality ({} bytes)",
            low.len(),
            high.len(),
        );
    }

    #[test]
    fn max_quality_encodes_losslessly_without_effort() {
        let codec = WebPCodec;
        let original = create_alpha_test_image(64, 48);
        let encoded = codec
            .encode(
                &original,
                &EncodeOptions {
                    quality: 100,
                    effort: None,
                    png_palette: Default::default(),
                    jxl_encoder: Default::default(),
                    threads: None,
                },
            )
            .expect("encode q100 failed");

        let decoded = codec.decode(&encoded).expect("decode q100 failed");
        assert_eq!(decoded.data, original.data);
    }

    #[test]
    fn max_quality_encodes_losslessly_with_effort() {
        let codec = WebPCodec;
        let original = create_alpha_test_image(64, 48);
        let encoded = codec
            .encode(
                &original,
                &EncodeOptions {
                    quality: 100,
                    effort: Some(50),
                    png_palette: Default::default(),
                    jxl_encoder: Default::default(),
                    threads: None,
                },
            )
            .expect("encode q100 failed");

        let decoded = codec.decode(&encoded).expect("decode q100 failed");
        assert_eq!(decoded.data, original.data);
    }
}
