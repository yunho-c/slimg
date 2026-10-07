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
    /// Encodes with the default settings, equivalent to effort 100.
    pub fn slimg_jpegli_encode_rgb(
        rgb: *const u8,
        width: u32,
        height: u32,
        quality: u8,
        out: *mut slimg_jpegli_result,
    ) -> i32;

    /// Encodes with effort 0..=100 (larger values behave as 100).
    ///
    /// 0..=24 uses sequential JPEG with fixed Huffman tables; 25..=49 uses
    /// sequential JPEG with optimized tables; 50..=74 uses progressive level 1;
    /// 75..=100 uses progressive level 2. Both progressive modes optimize tables.
    /// Effort does not change quality, subsampling or adaptive quantization.
    pub fn slimg_jpegli_encode_rgb_with_effort(
        rgb: *const u8,
        width: u32,
        height: u32,
        quality: u8,
        effort: u8,
        out: *mut slimg_jpegli_result,
    ) -> i32;

    pub fn slimg_jpegli_decode_rgba(
        data: *const u8,
        len: usize,
        out: *mut slimg_jpegli_result,
    ) -> i32;

    pub fn slimg_jpegli_free_result(result: *mut slimg_jpegli_result);
}
