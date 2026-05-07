use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData};
use crate::error::Result;

#[cfg(all(feature = "jpeg-backend-mozjpeg", feature = "jpeg-backend-jpegli"))]
compile_error!("jpeg-backend-mozjpeg and jpeg-backend-jpegli are mutually exclusive");

#[cfg(not(any(feature = "jpeg-backend-mozjpeg", feature = "jpeg-backend-jpegli")))]
compile_error!("one JPEG backend feature must be enabled");

#[cfg(feature = "jpeg-backend-mozjpeg")]
mod mozjpeg;

#[cfg(feature = "jpeg-backend-jpegli")]
mod jpegli;

/// JPEG codec routed to the selected backend implementation.
pub struct JpegCodec;

impl Codec for JpegCodec {
    fn format(&self) -> Format {
        Format::Jpeg
    }

    fn decode(&self, data: &[u8]) -> Result<ImageData> {
        backend::decode(data)
    }

    fn encode(&self, image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
        backend::encode(image, options)
    }
}

#[cfg(feature = "jpeg-backend-mozjpeg")]
use mozjpeg as backend;

#[cfg(feature = "jpeg-backend-jpegli")]
use jpegli as backend;

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
        let codec = JpegCodec;
        let original = create_test_image(64, 48);
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");

        assert!(
            encoded.len() >= 3,
            "encoded data too short: {} bytes",
            encoded.len()
        );
        assert_eq!(
            &encoded[..3],
            &[0xFF, 0xD8, 0xFF],
            "missing JPEG magic bytes"
        );

        let decoded = codec.decode(&encoded).expect("decode failed");
        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(
            decoded.data.len(),
            (decoded.width * decoded.height * 4) as usize
        );
    }

    #[test]
    fn encode_produces_smaller_at_lower_quality() {
        let codec = JpegCodec;
        let image = create_test_image(128, 96);

        let high = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 95,
                    effort: None,
                    threads: None,
                },
            )
            .expect("encode q95 failed");
        let low = codec
            .encode(
                &image,
                &EncodeOptions {
                    quality: 30,
                    effort: None,
                    threads: None,
                },
            )
            .expect("encode q30 failed");

        assert!(
            low.len() < high.len(),
            "low quality ({} bytes) should be smaller than high quality ({} bytes)",
            low.len(),
            high.len(),
        );
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let codec = JpegCodec;
        let result = codec.decode(b"not a jpeg");
        assert!(result.is_err(), "decoding invalid data should fail");
    }
}
