use crate::error::{Error, Result};

use super::{EncodeOptions, ImageData};

pub(super) fn decode(_data: &[u8]) -> Result<ImageData> {
    Err(Error::Decode(
        "jpegli backend is not implemented yet".to_string(),
    ))
}

pub(super) fn encode(_image: &ImageData, _options: &EncodeOptions) -> Result<Vec<u8>> {
    Err(Error::Encode(
        "jpegli backend is not implemented yet".to_string(),
    ))
}
