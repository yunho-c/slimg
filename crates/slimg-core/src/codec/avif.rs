use imgref::Img;
use rgb::RGBA8;

use crate::error::{Error, Result};
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};

/// AVIF codec backed by ravif for encoding.
///
/// Decoding is temporarily unavailable while native AVIF decode support is
/// removed from the dependency graph to avoid `dav1d-sys` macOS cross-build
/// failures.
pub struct AvifCodec;

impl Codec for AvifCodec {
    fn format(&self) -> Format {
        Format::Avif
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        let _ = data;
        Err(Error::Decode(
            "AVIF decode support is temporarily unavailable".to_string(),
        ))
    }

    fn encode(&self, image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
        let width = image.width as usize;
        let height = image.height as usize;

        // Convert raw RGBA bytes to Vec<RGBA8>
        let pixels: Vec<RGBA8> = image
            .data
            .chunks_exact(4)
            .map(|px| RGBA8::new(px[0], px[1], px[2], px[3]))
            .collect();

        let buffer = Img::new(pixels.as_slice(), width, height);

        let encoded = ravif::Encoder::new()
            .with_quality(options.quality as f32)
            .with_speed(6)
            .with_num_threads(options.threads.or(Some(1)))
            .encode_rgba(buffer)
            .map_err(|e| Error::Encode(format!("ravif encode: {e}")))?;

        Ok(encoded.avif_file)
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
    fn encode_produces_valid_avif() {
        let codec = AvifCodec;
        let image = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 80,
            threads: None,
        };

        let encoded = codec.encode(&image, &options).expect("encode failed");

        // Verify AVIF container: bytes 4-7 should be "ftyp"
        assert!(
            encoded.len() >= 8,
            "encoded data too short: {} bytes",
            encoded.len()
        );
        assert_eq!(&encoded[4..8], b"ftyp", "missing AVIF ftyp box");
    }

    #[test]
    fn decode_is_temporarily_unavailable() {
        let codec = AvifCodec;
        let image = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 80,
            threads: None,
        };

        let encoded = codec.encode(&image, &options).expect("encode failed");
        let err = codec.decode(&encoded).expect_err("decode should be unavailable");
        assert!(
            err.to_string()
                .contains("AVIF decode support is temporarily unavailable"),
            "unexpected decode error: {err}"
        );
    }
}
