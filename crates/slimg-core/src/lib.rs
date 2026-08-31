pub mod codec;
pub mod crop;
pub mod error;
pub mod extend;
pub mod format;
pub mod palette;
pub mod pipeline;
pub mod resize;

pub use codec::{Codec, EncodeOptions, ImageData, JxlEncoderPreference, PngPaletteMode};
pub use crop::CropMode;
pub use error::{Error, Result};
pub use extend::{ExtendMode, FillColor};
pub use format::Format;
pub use palette::{PaletteRecommendation, PaletteStats, analyze_palette_suitability};
pub use pipeline::{
    PipelineOptions, PipelineResult, convert, decode, decode_file, optimize, optimize_with_options,
    output_path,
};
pub use resize::ResizeMode;
