use std::ffi::c_char;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct slimg_jpegli_result {
    pub data: *mut u8,
    pub len: usize,
    pub width: u32,
    pub height: u32,
    pub status: i32,
    pub error_message: *mut c_char,
}

unsafe extern "C" {
    pub fn slimg_jpegli_encode_rgb(
        rgb: *const u8,
        width: u32,
        height: u32,
        quality: u8,
        out: *mut slimg_jpegli_result,
    ) -> i32;

    pub fn slimg_jpegli_decode_rgba(
        data: *const u8,
        len: usize,
        out: *mut slimg_jpegli_result,
    ) -> i32;

    pub fn slimg_jpegli_free_result(result: *mut slimg_jpegli_result);
}
