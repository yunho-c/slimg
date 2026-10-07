use std::ffi::CStr;
use std::ptr;

use jpegli_sys::{
    slimg_jpegli_decode_rgba, slimg_jpegli_encode_rgb_with_effort, slimg_jpegli_free_result,
    slimg_jpegli_result,
};

use super::{EncodeOptions, ImageData};
use crate::error::{Error, Result};

pub(super) fn decode(data: &[u8]) -> Result<ImageData> {
    let mut output = empty_result();
    let status = unsafe { slimg_jpegli_decode_rgba(data.as_ptr(), data.len(), &mut output) };
    if status != 0 {
        let message = result_error_message(&output)
            .unwrap_or_else(|| "jpegli decode failed without an error message".to_string());
        free_result(&mut output);
        return Err(Error::Decode(message));
    }

    let width = output.width;
    let height = output.height;
    let bytes = copy_result_bytes(&output);
    free_result(&mut output);
    let bytes =
        bytes.ok_or_else(|| Error::Decode("jpegli decode returned no image bytes".to_string()))?;
    Ok(ImageData::new(width, height, bytes))
}

pub(super) fn encode(image: &ImageData, options: &EncodeOptions) -> Result<Vec<u8>> {
    let rgb = image.to_rgb();
    let mut output = empty_result();
    let status = unsafe {
        slimg_jpegli_encode_rgb_with_effort(
            rgb.as_ptr(),
            image.width,
            image.height,
            options.quality,
            options.effort.unwrap_or(100),
            &mut output,
        )
    };
    if status != 0 {
        let message = result_error_message(&output)
            .unwrap_or_else(|| "jpegli encode failed without an error message".to_string());
        free_result(&mut output);
        return Err(Error::Encode(message));
    }

    let bytes = copy_result_bytes(&output);
    free_result(&mut output);
    let bytes =
        bytes.ok_or_else(|| Error::Encode("jpegli encode returned no output bytes".to_string()))?;
    Ok(bytes)
}

fn empty_result() -> slimg_jpegli_result {
    slimg_jpegli_result {
        data: ptr::null_mut(),
        len: 0,
        width: 0,
        height: 0,
        status: 0,
        error_message: ptr::null_mut(),
    }
}

fn result_error_message(result: &slimg_jpegli_result) -> Option<String> {
    if result.error_message.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(result.error_message) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn copy_result_bytes(result: &slimg_jpegli_result) -> Option<Vec<u8>> {
    if result.data.is_null() || result.len == 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(result.data, result.len) }.to_vec())
}

fn free_result(result: &mut slimg_jpegli_result) {
    unsafe {
        slimg_jpegli_free_result(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Read length-delimited marker payloads, skipping entropy bytes and byte
    // stuffing. Scanning raw bytes for SOF/SOS would also match marker payloads.
    fn segments(jpeg: &[u8]) -> Vec<(u8, &[u8])> {
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        let mut result = Vec::new();
        let mut pos = 2;
        while pos < jpeg.len() {
            if jpeg[pos] != 0xff {
                pos += 1;
                continue;
            }
            while jpeg[pos] == 0xff {
                pos += 1;
            }
            let marker = jpeg[pos];
            pos += 1;
            match marker {
                0x00 | 0xd0..=0xd7 => continue,
                0xd9 => return result,
                _ => {}
            }
            let len = u16::from_be_bytes([jpeg[pos], jpeg[pos + 1]]) as usize;
            assert!(len >= 2);
            result.push((marker, &jpeg[pos + 2..pos + len]));
            pos += len;
        }
        panic!("JPEG is missing EOI");
    }

    fn image(width: u32, height: u32) -> ImageData {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[
                    (x * 7 + y * 3) as u8,
                    (x * 2 + y * 11) as u8,
                    (x * y) as u8,
                    255,
                ]);
            }
        }
        ImageData::new(width, height, data)
    }

    #[test]
    fn effort_changes_jpeg_structure_and_preserves_quality() {
        let image = image(65, 49);
        let options = EncodeOptions {
            quality: 80,
            ..Default::default()
        };
        let default = encode(&image, &options).unwrap();
        let reference = decode(&default).unwrap();
        let default_segments = segments(&default);
        let payloads = |marker| {
            default_segments
                .iter()
                .filter(|(m, _)| *m == marker)
                .map(|(_, data)| *data)
                .collect::<Vec<_>>()
        };
        let mut sequential_tables = Vec::new();
        for (efforts, frame, scans) in [
            (&[0, 24][..], 0xc0, 1),
            (&[25, 49][..], 0xc0, 1),
            // Default subsampling uses separate DC scans for each component.
            (&[50, 74][..], 0xc2, 9),
            (&[75, 100, 101, 255][..], 0xc2, 15),
        ] {
            let mut first = None;
            for &effort in efforts {
                let encoded = encode(
                    &image,
                    &EncodeOptions {
                        effort: Some(effort),
                        ..options.clone()
                    },
                )
                .unwrap();
                let markers = segments(&encoded);
                let frames: Vec<_> = markers
                    .iter()
                    .filter(|(m, _)| *m == frame)
                    .map(|(_, data)| *data)
                    .collect();
                // Same components, dimensions, sampling factors and quantizers.
                assert_eq!(frames, payloads(0xc2), "effort {effort}");
                assert_eq!(markers.iter().filter(|(m, _)| *m == 0xda).count(), scans);
                let quant_tables: Vec<_> = markers
                    .iter()
                    .filter(|(m, _)| *m == 0xdb)
                    .map(|(_, data)| *data)
                    .collect();
                assert_eq!(quant_tables, payloads(0xdb), "effort {effort}");
                let decoded = decode(&encoded).unwrap();
                assert_eq!((decoded.width, decoded.height), (image.width, image.height));
                assert_eq!(decoded.data, reference.data, "effort {effort}");
                if effort >= 75 {
                    assert_eq!(encoded, default, "default settings changed");
                }
                if let Some(first) = &first {
                    assert_eq!(&encoded, first, "settings changed within an effort tier");
                } else {
                    if scans == 1 {
                        sequential_tables.push(
                            markers
                                .iter()
                                .filter(|(m, _)| *m == 0xc4)
                                .map(|(_, data)| data.to_vec())
                                .collect::<Vec<_>>(),
                        );
                    }
                    first = Some(encoded);
                }
            }
        }
        // The two sequential tiers must differ in actual Huffman tables.
        assert_ne!(sequential_tables[0], sequential_tables[1]);
    }

    #[test]
    fn effort_roundtrips_tiny_images_at_quality_extremes() {
        for (width, height) in [(1, 1), (17, 9)] {
            let image = image(width, height);
            for quality in [0, 100] {
                for effort in [0, 25, 50, 75] {
                    let encoded = encode(
                        &image,
                        &EncodeOptions {
                            quality,
                            effort: Some(effort),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let decoded = decode(&encoded).unwrap();
                    assert_eq!((decoded.width, decoded.height), (width, height));
                    assert_eq!(decoded.data.len(), (width * height * 4) as usize);
                }
            }
        }
    }
}
