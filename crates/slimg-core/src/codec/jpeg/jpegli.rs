use std::ffi::CStr;
use std::ptr;

use libjxl_sys::jpegli::{
    slimg_jpegli_decode_rgba, slimg_jpegli_encode_rgb, slimg_jpegli_free_result,
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
        slimg_jpegli_encode_rgb(
            rgb.as_ptr(),
            image.width,
            image.height,
            options.quality,
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
