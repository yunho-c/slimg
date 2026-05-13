/// JXL encoding configuration.
pub(crate) struct EncodeConfig {
    pub lossless: bool,
    pub distance: f32,
    pub effort: Option<u8>,
    pub threads: Option<usize>,
}

impl EncodeConfig {
    pub fn from_options(quality: u8, effort: Option<u8>, threads: Option<usize>) -> Self {
        if quality >= 100 {
            return Self {
                lossless: true,
                distance: 0.0,
                effort,
                threads,
            };
        }
        let distance = unsafe { libjxl_sys::JxlEncoderDistanceFromQuality(quality as f32) };
        Self {
            lossless: false,
            distance,
            effort,
            threads,
        }
    }
}
