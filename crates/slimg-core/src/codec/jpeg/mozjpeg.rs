use crate::error::{Error, Result};

use super::{EncodeOptions, ImageData};

pub(super) fn decode(data: &[u8]) -> Result<ImageData> {
    // mozjpeg uses setjmp/longjmp internally, which translates to panics
    // in Rust. We must catch those to turn them into proper errors.
    let data = data.to_vec();
    let result = std::panic::catch_unwind(move || -> Result<ImageData> {
        let decompress = mozjpeg::Decompress::new_mem(&data)
            .map_err(|e| Error::Decode(format!("mozjpeg decompress init: {e}")))?;

        let width = decompress.width() as u32;
        let height = decompress.height() as u32;

        let mut decompressor = decompress
            .rgba()
            .map_err(|e| Error::Decode(format!("mozjpeg rgba conversion: {e}")))?;

        let pixels: Vec<[u8; 4]> = decompressor
            .read_scanlines()
            .map_err(|e| Error::Decode(format!("mozjpeg read scanlines: {e}")))?;

        decompressor
            .finish()
            .map_err(|e| Error::Decode(format!("mozjpeg finish: {e}")))?;

        let rgba_data: Vec<u8> = pixels.into_iter().flatten().collect();

        Ok(ImageData::new(width, height, rgba_data))
    });

    match result {
        Ok(inner) => inner,
        Err(panic) => {
            let msg = panic_message(&panic);
            Err(Error::Decode(format!("mozjpeg panicked: {msg}")))
        }
    }
}

pub(super) fn encode(image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
    let width = image.width;
    let height = image.height;
    let rgb_data = image.to_rgb();
    let quality = options.quality as f32;
    let effort = options.effort;

    let result = std::panic::catch_unwind(move || -> Result<Vec<u8>> {
        let mut compress = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);

        compress.set_size(width as usize, height as usize);
        compress.set_quality(quality);
        if effort.map(|value| value >= 25).unwrap_or(true) {
            compress.set_progressive_mode();
        }
        compress.set_optimize_scans(effort.map(|value| value >= 50).unwrap_or(true));
        compress.set_optimize_coding(effort.map(|value| value >= 25).unwrap_or(true));

        let mut compressor = compress
            .start_compress(Vec::new())
            .map_err(|e| Error::Encode(format!("mozjpeg compress start: {e}")))?;

        compressor
            .write_scanlines(&rgb_data)
            .map_err(|e| Error::Encode(format!("mozjpeg write scanlines: {e}")))?;

        let output = compressor
            .finish()
            .map_err(|e| Error::Encode(format!("mozjpeg finish: {e}")))?;

        Ok(output)
    });

    match result {
        Ok(inner) => inner,
        Err(panic) => {
            let msg = panic_message(&panic);
            Err(Error::Encode(format!("mozjpeg panicked: {msg}")))
        }
    }
}

/// Extract a human-readable message from a `catch_unwind` panic payload.
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
