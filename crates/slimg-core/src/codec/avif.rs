use imgref::Img;
use rgb::RGBA8;
use zenavif::{DecoderConfig, Unstoppable};
use zenpixels_convert::PixelBufferConvertTypedExt;

use crate::error::{Error, Result};
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};

/// AVIF codec backed by zenavif for decoding and ravif for encoding.
pub struct AvifCodec;

impl Codec for AvifCodec {
    fn format(&self) -> Format {
        Format::Avif
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        let config = DecoderConfig::new().prefer_8bit(true);
        let decoded = zenavif::decode_with(data, &config, &Unstoppable)
            .map_err(|e| Error::Decode(format!("zenavif decode: {e}")))?;
        let width = decoded.width();
        let height = decoded.height();
        let rgba = decoded.to_rgba8();

        Ok(ImageData::new(
            width,
            height,
            rgba.copy_to_contiguous_bytes(),
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

        let ravif_quality = options.quality.max(1) as f32;
        let encoded = ravif::Encoder::new()
            .with_quality(ravif_quality)
            .with_speed(options.effort.map(effort_to_ravif_speed).unwrap_or(6))
            .with_num_threads(options.threads.or(Some(1)))
            .encode_rgba(buffer)
            .map_err(|e| Error::Encode(format!("ravif encode: {e}")))?;

        Ok(encoded.avif_file)
    }
}

fn effort_to_ravif_speed(effort: u8) -> u8 {
    let effort = effort.min(100) as u16;
    if effort <= 50 {
        (10 - ((effort * 4 + 25) / 50)) as u8
    } else {
        (6 - (((effort - 50) * 5 + 25) / 50)) as u8
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
            effort: None,
            png_palette: Default::default(),
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
    fn encode_and_decode_returns_rgba() {
        let codec = AvifCodec;
        let image = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 80,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&image, &options).expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.width, image.width);
        assert_eq!(decoded.height, image.height);
        assert_eq!(decoded.data.len(), (64 * 48 * 4) as usize);
    }

    #[test]
    fn encode_accepts_zero_quality() {
        let codec = AvifCodec;
        let image = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 0,
            effort: None,
            png_palette: Default::default(),
            threads: None,
        };

        let encoded = codec.encode(&image, &options).expect("encode failed");
        assert!(!encoded.is_empty());
    }
}
