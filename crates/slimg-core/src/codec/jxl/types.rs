/// JXL encoding configuration.
pub(crate) struct EncodeConfig {
    pub lossless: bool,
    pub distance: f32,
    pub threads: Option<usize>,
}

impl EncodeConfig {
    pub fn from_quality(quality: u8, threads: Option<usize>) -> Self {
        if quality >= 100 {
            return Self {
                lossless: true,
                distance: 0.0,
                threads,
            };
        }
        let distance = unsafe { libjxl_sys::JxlEncoderDistanceFromQuality(quality as f32) };
        Self {
            lossless: false,
            distance,
            threads,
        }
    }
}
