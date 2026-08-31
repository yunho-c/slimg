mod decoder;
mod encoder;
mod types;

use crate::error::Result;
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};

/// JXL codec backed by libjxl (BSD-3-Clause) for both encoding and decoding.
pub struct JxlCodec;

impl Codec for JxlCodec {
    fn format(&self) -> Format {
        Format::Jxl
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        let mut dec = decoder::Decoder::new()?;
        let (width, height, pixels) = dec.decode_to_rgba(data)?;
        Ok(ImageData::new(width, height, pixels))
    }

    fn encode(&self, image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
        let config =
            types::EncodeConfig::from_options(options.quality, options.effort, options.threads);
        let mut enc = encoder::Encoder::new()?;

        // Fully opaque images don't need an alpha channel; encoding RGB
        // avoids storing a redundant plane.
        let is_opaque = image.data.chunks_exact(4).all(|px| px[3] == u8::MAX);
        if is_opaque {
            enc.encode(&image.to_rgb(), image.width, image.height, 3, &config)
        } else {
            enc.encode(&image.data, image.width, image.height, 4, &config)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_image(width: u32, height: u32) -> ImageData {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / width.max(1)) as u8;
                let g = ((y * 255) / height.max(1)) as u8;
                let b = 128u8;
                let a = 255u8;
                pixels.extend_from_slice(&[r, g, b, a]);
            }
        }
        ImageData::new(width, height, pixels)
    }

    #[test]
    fn encode_lossy_produces_valid_jxl() {
        let codec = JxlCodec;
        let image = create_test_image(8, 8);
        let options = EncodeOptions {
            quality: 80,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec
            .encode(&image, &options)
            .expect("encode should succeed");
        assert!(!encoded.is_empty(), "encoded data should not be empty");

        // Check JXL magic bytes (bare codestream: 0xFF 0x0A)
        assert!(
            (encoded.len() >= 2 && encoded[0] == 0xFF && encoded[1] == 0x0A)
                || (encoded.len() >= 8
                    && encoded[..4] == [0x00, 0x00, 0x00, 0x0C]
                    && &encoded[4..8] == b"JXL "),
            "output should have a valid JXL signature"
        );
    }

    #[test]
    fn encode_lossless_produces_valid_jxl() {
        let codec = JxlCodec;
        let image = create_test_image(8, 8);
        let options = EncodeOptions {
            quality: 100,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec
            .encode(&image, &options)
            .expect("lossless encode should succeed");
        assert!(!encoded.is_empty());
    }

    #[test]
    fn explicit_thread_budget_matches_libjxl_build_capability() {
        let codec = JxlCodec;
        let image = create_test_image(8, 8);
        let result = codec.encode(
            &image,
            &EncodeOptions {
                quality: 80,
                effort: None,
                png_palette: Default::default(),
                threads: Some(2),
            },
        );

        if libjxl_sys::JXL_THREADS_AVAILABLE {
            assert!(result.is_ok(), "threaded source/prebuilt encode failed");
        } else {
            let error = result.expect_err("old serial-only prebuilts must reject a thread budget");
            assert!(
                error
                    .to_string()
                    .contains("does not include the thread runner")
            );
        }
    }

    #[test]
    fn roundtrip_lossy() {
        let codec = JxlCodec;
        let original = create_test_image(16, 16);
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(decoded.data.len(), original.data.len());
    }

    #[test]
    fn roundtrip_lossless_preserves_alpha() {
        let codec = JxlCodec;
        let mut image = create_test_image(4, 4);
        for (i, px) in image.data.chunks_exact_mut(4).enumerate() {
            px[3] = (i * 16) as u8;
        }
        let original = image.clone();

        let encoded = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 100,
                    effort: None,
                    png_palette: Default::default(),
                    threads: None,
                },
            )
            .expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.data, original.data);
    }

    #[test]
    fn opaque_image_decodes_back_opaque() {
        let codec = JxlCodec;
        let image = create_test_image(8, 8); // alpha = 255 everywhere

        let encoded = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 100,
                    effort: None,
                    png_palette: Default::default(),
                    threads: None,
                },
            )
            .expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.width, 8);
        assert_eq!(decoded.height, 8);
        assert!(decoded.data.chunks_exact(4).all(|px| px[3] == u8::MAX));
    }

    #[test]
    fn decode_truncated_data_returns_error() {
        let codec = JxlCodec;
        let image = create_test_image(16, 16);
        let encoded = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 80,
                    effort: None,
                    png_palette: Default::default(),
                    threads: None,
                },
            )
            .expect("encode failed");

        // Cut the codestream short; the decoder must error out instead
        // of spinning forever waiting for more input.
        let truncated = &encoded[..encoded.len() / 2];
        assert!(codec.decode(truncated).is_err());
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let codec = JxlCodec;
        // Valid signature followed by garbage.
        let mut data = vec![0xFF, 0x0A];
        data.extend_from_slice(&[0xAB; 64]);
        assert!(codec.decode(&data).is_err());
    }

    #[test]
    fn roundtrip_lossless() {
        let codec = JxlCodec;
        let original = create_test_image(4, 4);
        let options = EncodeOptions {
            quality: 100,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(
            decoded.data, original.data,
            "lossless roundtrip should produce identical pixels"
        );
    }
}
