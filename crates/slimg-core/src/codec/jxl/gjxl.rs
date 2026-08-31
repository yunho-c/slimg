use std::sync::OnceLock;

use gjxl::{Backend, Context, EncoderOptions, ErrorKind, ImageView};

use crate::error::{Error, Result};

use super::encoder::effort_to_jxl_effort;
use super::{EncodeOptions, ImageData, JxlFallbackReason};

pub(super) enum GjxlAttempt {
    Encoded(Vec<u8>),
    Fallback(JxlFallbackReason),
}

static CONTEXT: OnceLock<std::result::Result<Context, gjxl::Error>> = OnceLock::new();

pub(super) fn try_encode(image: &ImageData, options: &EncodeOptions) -> Result<GjxlAttempt> {
    let context = match CONTEXT.get_or_init(|| Context::new(Backend::Auto)) {
        Ok(context) => context,
        Err(error) => return classify_failure(error.kind(), error.message().to_string()),
    };

    let row_stride_bytes = (image.width as usize)
        .checked_mul(4)
        .ok_or_else(|| Error::Encode("GJXL row stride overflow".into()))?;
    let image = ImageView::rgba8(image.width, image.height, row_stride_bytes, &image.data)
        .map_err(native_error)?;
    let encoder_options = EncoderOptions {
        distance: gjxl::distance_from_quality(options.quality as f32),
        effort: options
            .effort
            .map(effort_to_jxl_effort)
            .map(i32::from)
            .unwrap_or(7),
    };

    match context.encode(&image, encoder_options) {
        Ok(data) => Ok(GjxlAttempt::Encoded(data)),
        Err(error) => classify_failure(error.kind(), error.message().to_string()),
    }
}

fn classify_failure(kind: ErrorKind, detail: String) -> Result<GjxlAttempt> {
    match kind {
        ErrorKind::Unsupported => Ok(GjxlAttempt::Fallback(JxlFallbackReason::Unsupported(
            detail,
        ))),
        ErrorKind::Unavailable => Ok(GjxlAttempt::Fallback(JxlFallbackReason::Unavailable(
            detail,
        ))),
        _ => Err(Error::Encode(format!("GJXL {kind:?}: {detail}"))),
    }
}

fn native_error(error: gjxl::Error) -> Error {
    Error::Encode(format!("GJXL {:?}: {}", error.kind(), error.message()))
}

#[cfg(test)]
pub(super) fn context_address() -> Result<usize> {
    match CONTEXT.get_or_init(|| Context::new(Backend::Auto)) {
        Ok(context) => Ok(std::ptr::from_ref(context) as usize),
        Err(error) => Err(native_error(error.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unsupported_and_unavailable_fall_back() {
        let unsupported = classify_failure(ErrorKind::Unsupported, "unsupported".into())
            .expect("unsupported should fall back");
        assert!(matches!(
            unsupported,
            GjxlAttempt::Fallback(JxlFallbackReason::Unsupported(_))
        ));

        let unavailable = classify_failure(ErrorKind::Unavailable, "unavailable".into())
            .expect("unavailable should fall back");
        assert!(matches!(
            unavailable,
            GjxlAttempt::Fallback(JxlFallbackReason::Unavailable(_))
        ));

        for kind in [
            ErrorKind::InvalidArgument,
            ErrorKind::OutOfMemory,
            ErrorKind::Backend,
            ErrorKind::Internal,
        ] {
            assert!(
                classify_failure(kind, "fatal".into()).is_err(),
                "{kind:?} must not silently fall back"
            );
        }
    }

    #[test]
    fn quality_mapping_comes_from_gjxl() {
        assert_eq!(gjxl::distance_from_quality(100.0), 0.0);
        assert!((gjxl::distance_from_quality(90.0) - 1.0).abs() < f32::EPSILON);
        assert!((gjxl::distance_from_quality(80.0) - 1.9).abs() < f32::EPSILON);
    }
}
