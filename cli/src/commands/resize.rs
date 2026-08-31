use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use slimg_core::{PipelineOptions, ResizeMode, convert, decode_file, output_path};

use super::{FileOutcome, FormatArg, run_batch, safe_write};

#[derive(Debug, Args)]
pub struct ResizeArgs {
    /// Input file or directory
    pub input: PathBuf,

    /// Target width in pixels
    #[arg(long)]
    pub width: Option<u32>,

    /// Target height in pixels
    #[arg(long)]
    pub height: Option<u32>,

    /// Scale factor (e.g. 0.5 for half size)
    #[arg(long)]
    pub scale: Option<f64>,

    /// Output format (defaults to input format)
    #[arg(short, long)]
    pub format: Option<FormatArg>,

    /// Encoding quality (0-100)
    #[arg(short, long, default_value_t = 80)]
    pub quality: u8,

    /// Encoding effort (0-100, higher is slower/smaller)
    #[arg(short, long)]
    pub effort: Option<u8>,

    /// Output path (file or directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Overwrite existing files
    #[arg(long)]
    pub overwrite: bool,

    /// Process subdirectories recursively
    #[arg(long)]
    pub recursive: bool,

    /// Number of parallel jobs (defaults to CPU count)
    #[arg(short, long)]
    pub jobs: Option<usize>,
}

fn build_resize_mode(args: &ResizeArgs) -> anyhow::Result<ResizeMode> {
    match (args.width, args.height, args.scale) {
        (Some(w), Some(h), None) => Ok(ResizeMode::Fit(w, h)),
        (Some(w), None, None) => Ok(ResizeMode::Width(w)),
        (None, Some(h), None) => Ok(ResizeMode::Height(h)),
        (None, None, Some(s)) => Ok(ResizeMode::Scale(s)),
        (None, None, None) => {
            anyhow::bail!("specify at least one of --width, --height, or --scale");
        }
        _ => {
            anyhow::bail!("--scale cannot be combined with --width or --height");
        }
    }
}

pub fn run(args: ResizeArgs) -> anyhow::Result<()> {
    let resize_mode = build_resize_mode(&args)?;

    run_batch(
        &args.input,
        args.output.as_deref(),
        args.recursive,
        args.jobs,
        "resize",
        |file, output| {
            let original_size = std::fs::metadata(file)?.len();
            let (image, src_format) =
                decode_file(file).with_context(|| format!("{}", file.display()))?;

            let target_format = args.format.map(|f| f.into_format()).unwrap_or(src_format);

            if !target_format.can_encode() {
                anyhow::bail!("cannot encode to {} format", target_format.extension());
            }

            let options = PipelineOptions {
                format: target_format,
                quality: args.quality,
                effort: args.effort,
                png_palette: Default::default(),
                threads: None,
                resize: Some(resize_mode.clone()),
                crop: None,
                extend: None,
                fill_color: None,
            };

            let result =
                convert(&image, &options).with_context(|| format!("{}", file.display()))?;

            let out = output_path(file, target_format, output);
            safe_write(&out, &result.data, args.overwrite)?;

            Ok(FileOutcome::Written {
                out,
                original_size,
                new_size: result.data.len() as u64,
            })
        },
    )
}
