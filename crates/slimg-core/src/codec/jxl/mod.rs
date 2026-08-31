mod decoder;
mod encoder;
#[cfg(feature = "jxl-encoder-gjxl")]
mod gjxl;
mod types;

use crate::error::{Error, Result};
use crate::format::Format;

use super::{Codec, EncodeOptions, ImageData, JxlEncoderPreference};

/// Encoder implementation that produced a JPEG XL codestream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JxlEncodeBackend {
    /// The experimental GJXL C API.
    Gjxl,
    /// The established libjxl encoder.
    Libjxl,
}

/// Why an encode used libjxl instead of the optional GJXL backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JxlFallbackReason {
    /// Slimg was built without the experimental GJXL feature.
    FeatureDisabled,
    /// Quality 100 requests lossless output, which GJXL does not support yet.
    Lossless,
    /// The image contains at least one non-opaque alpha sample.
    NonOpaqueAlpha,
    /// A thread budget was requested, which GJXL cannot currently honor.
    ThreadBudget,
    /// GJXL reported that the requested capability is unsupported.
    Unsupported(String),
    /// GJXL could not initialize its requested execution backend.
    Unavailable(String),
}

/// Encoded JPEG XL bytes together with backend-selection diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JxlEncodeOutcome {
    /// Encoded JPEG XL bytes.
    pub data: Vec<u8>,
    /// Encoder that produced `data`.
    pub backend: JxlEncodeBackend,
    /// Reason libjxl was selected instead of an enabled GJXL backend.
    pub fallback_reason: Option<JxlFallbackReason>,
}

/// Whether this build includes the experimental GJXL encoder.
pub const fn gjxl_backend_compiled() -> bool {
    cfg!(feature = "jxl-encoder-gjxl")
}

/// JXL codec backed by libjxl for decoding and by the selected encoder route.
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
        Ok(encode_with_diagnostics(image, options)?.data)
    }
}

/// Encode JPEG XL while reporting whether GJXL or libjxl produced the bytes.
///
/// The ordinary [`Codec::encode`] path intentionally discards this diagnostic
/// metadata. Benchmarks and experiments should use this function so a libjxl
/// fallback cannot be mistaken for a GJXL sample.
pub fn encode_with_diagnostics(
    image: &ImageData,
    options: &EncodeOptions,
) -> Result<JxlEncodeOutcome> {
    validate_image_layout(image)?;

    if options.jxl_encoder == JxlEncoderPreference::Libjxl {
        return encode_with_libjxl(image, options, None);
    }

    #[cfg(feature = "jxl-encoder-gjxl")]
    {
        if let Some(reason) = gjxl_preflight_fallback(image, options) {
            return encode_with_libjxl(image, options, Some(reason));
        }

        match gjxl::try_encode(image, options)? {
            gjxl::GjxlAttempt::Encoded(data) => Ok(JxlEncodeOutcome {
                data,
                backend: JxlEncodeBackend::Gjxl,
                fallback_reason: None,
            }),
            gjxl::GjxlAttempt::Fallback(reason) => encode_with_libjxl(image, options, Some(reason)),
        }
    }

    #[cfg(not(feature = "jxl-encoder-gjxl"))]
    encode_with_libjxl(image, options, Some(JxlFallbackReason::FeatureDisabled))
}

fn encode_with_libjxl(
    image: &ImageData,
    options: &EncodeOptions,
    fallback_reason: Option<JxlFallbackReason>,
) -> Result<JxlEncodeOutcome> {
    let config =
        types::EncodeConfig::from_options(options.quality, options.effort, options.threads);
    let mut enc = encoder::Encoder::new()?;
    let data = enc.encode_rgba(&image.data, image.width, image.height, &config)?;
    Ok(JxlEncodeOutcome {
        data,
        backend: JxlEncodeBackend::Libjxl,
        fallback_reason,
    })
}

fn validate_image_layout(image: &ImageData) -> Result<()> {
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::Encode("JXL image dimensions overflow the RGBA layout".into()))?;
    if image.width == 0 || image.height == 0 {
        return Err(Error::Encode("JXL image dimensions must be nonzero".into()));
    }
    if image.data.len() != expected {
        return Err(Error::Encode(format!(
            "JXL image data length mismatch: expected {expected} bytes, got {}",
            image.data.len()
        )));
    }
    Ok(())
}

#[cfg(feature = "jxl-encoder-gjxl")]
fn gjxl_preflight_fallback(
    image: &ImageData,
    options: &EncodeOptions,
) -> Option<JxlFallbackReason> {
    if options.quality >= 100 {
        return Some(JxlFallbackReason::Lossless);
    }
    if image.data.chunks_exact(4).any(|pixel| pixel[3] != 255) {
        return Some(JxlFallbackReason::NonOpaqueAlpha);
    }
    if options.threads.is_some() {
        return Some(JxlFallbackReason::ThreadBudget);
    }
    None
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

    fn options(quality: u8) -> EncodeOptions {
        EncodeOptions {
            quality,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: JxlEncoderPreference::PreferGjxl,
            threads: None,
        }
    }

    fn libjxl_options(quality: u8) -> EncodeOptions {
        EncodeOptions {
            jxl_encoder: JxlEncoderPreference::Libjxl,
            ..options(quality)
        }
    }

    #[test]
    fn effort_mapping_covers_low_default_and_high_tiers() {
        assert_eq!(encoder::effort_to_jxl_effort(0), 1);
        assert_eq!(encoder::effort_to_jxl_effort(50), 7);
        assert_eq!(encoder::effort_to_jxl_effort(100), 10);
    }

    #[test]
    fn malformed_image_layout_is_rejected_before_native_code() {
        let mut image = create_test_image(2, 2);
        image.data.pop();
        let error =
            encode_with_diagnostics(&image, &options(80)).expect_err("short RGBA data must fail");
        assert!(error.to_string().contains("data length mismatch"));
    }

    #[cfg(not(feature = "jxl-encoder-gjxl"))]
    #[test]
    fn diagnostics_report_disabled_gjxl_feature() {
        let outcome = encode_with_diagnostics(&create_test_image(8, 8), &options(80))
            .expect("libjxl fallback should encode");
        assert_eq!(outcome.backend, JxlEncodeBackend::Libjxl);
        assert_eq!(
            outcome.fallback_reason,
            Some(JxlFallbackReason::FeatureDisabled)
        );
    }

    #[test]
    fn explicit_libjxl_preference_is_not_a_fallback() {
        let outcome = encode_with_diagnostics(&create_test_image(8, 8), &libjxl_options(80))
            .expect("explicit libjxl encode should succeed");
        assert_eq!(outcome.backend, JxlEncodeBackend::Libjxl);
        assert_eq!(outcome.fallback_reason, None);
    }

    #[test]
    fn capability_matches_the_build_feature() {
        assert_eq!(gjxl_backend_compiled(), cfg!(feature = "jxl-encoder-gjxl"));
    }

    #[cfg(feature = "jxl-encoder-gjxl")]
    #[test]
    fn diagnostics_report_gjxl_for_eligible_input() {
        let image = create_test_image(16, 16);
        let outcome = encode_with_diagnostics(&image, &options(80))
            .expect("GJXL should encode eligible input");
        assert_eq!(outcome.backend, JxlEncodeBackend::Gjxl);
        assert_eq!(outcome.fallback_reason, None);

        let decoded = JxlCodec
            .decode(&outcome.data)
            .expect("libjxl should decode GJXL output");
        assert_eq!((decoded.width, decoded.height), (image.width, image.height));
        assert_eq!(decoded.data.len(), image.data.len());
    }

    #[cfg(feature = "jxl-encoder-gjxl")]
    #[test]
    fn lossless_alpha_and_thread_requests_report_fallback() {
        let opaque = create_test_image(8, 8);
        let lossless = encode_with_diagnostics(&opaque, &options(100))
            .expect("libjxl lossless fallback should encode");
        assert_eq!(lossless.backend, JxlEncodeBackend::Libjxl);
        assert_eq!(lossless.fallback_reason, Some(JxlFallbackReason::Lossless));

        let mut transparent = opaque.clone();
        transparent.data[3] = 128;
        let alpha = encode_with_diagnostics(&transparent, &options(80))
            .expect("libjxl alpha fallback should encode");
        assert_eq!(alpha.backend, JxlEncodeBackend::Libjxl);
        assert_eq!(
            alpha.fallback_reason,
            Some(JxlFallbackReason::NonOpaqueAlpha)
        );

        let mut threaded_options = options(80);
        threaded_options.threads = Some(1);
        let threaded = encode_with_diagnostics(&opaque, &threaded_options)
            .expect("libjxl thread-budget fallback should encode");
        assert_eq!(threaded.backend, JxlEncodeBackend::Libjxl);
        assert_eq!(
            threaded.fallback_reason,
            Some(JxlFallbackReason::ThreadBudget)
        );
    }

    #[cfg(feature = "jxl-encoder-gjxl")]
    #[test]
    fn gjxl_context_is_reused_across_concurrent_encodes() {
        let first_context = gjxl::context_address().expect("GJXL context should initialize");
        let image = create_test_image(16, 16);
        let handles = (0..4)
            .map(|_| {
                let image = image.clone();
                std::thread::spawn(move || {
                    let outcome = encode_with_diagnostics(&image, &options(80))?;
                    if outcome.backend != JxlEncodeBackend::Gjxl {
                        return Err(Error::Encode("concurrent encode fell back".into()));
                    }
                    Ok(outcome.data)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            let data = handle
                .join()
                .expect("encode thread should not panic")
                .expect("concurrent GJXL encode should succeed");
            assert!(data.starts_with(&[0xff, 0x0a]));
        }
        let second_context = gjxl::context_address().expect("GJXL context should remain available");
        assert_eq!(first_context, second_context);
    }

    #[test]
    fn encode_lossy_produces_valid_jxl() {
        let codec = JxlCodec;
        let image = create_test_image(8, 8);
        let options = options(80);

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
            jxl_encoder: JxlEncoderPreference::PreferGjxl,
            threads: None,
        };

        let encoded = codec
            .encode(&image, &options)
            .expect("lossless encode should succeed");
        assert!(!encoded.is_empty());
    }

    #[test]
    fn roundtrip_lossy() {
        let codec = JxlCodec;
        let original = create_test_image(16, 16);
        let options = EncodeOptions {
            quality: 90,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: JxlEncoderPreference::Libjxl,
            threads: None,
        };

        let encoded = codec.encode(&original, &options).expect("encode failed");
        let decoded = codec.decode(&encoded).expect("decode failed");

        assert_eq!(decoded.width, original.width);
        assert_eq!(decoded.height, original.height);
        assert_eq!(decoded.data.len(), original.data.len());
    }

    #[test]
    fn roundtrip_lossless() {
        let codec = JxlCodec;
        let original = create_test_image(4, 4);
        let options = EncodeOptions {
            quality: 100,
            effort: None,
            png_palette: Default::default(),
            jxl_encoder: JxlEncoderPreference::Libjxl,
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
