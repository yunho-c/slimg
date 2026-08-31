use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use slimg_core::{PipelineOptions, convert, decode_file, output_path};

use super::{FileOutcome, FormatArg, run_batch, safe_write};

#[derive(Debug, Args)]
pub struct ConvertArgs {
    /// Input file or directory
    pub input: PathBuf,

    /// Output format
    #[arg(short, long)]
    pub format: FormatArg,

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

pub fn run(args: ConvertArgs) -> anyhow::Result<()> {
    let target_format = args.format.into_format();

    if !target_format.can_encode() {
        anyhow::bail!("cannot encode to {} format", target_format.extension());
    }

    let options = PipelineOptions {
        format: target_format,
        quality: args.quality,
        effort: args.effort,
        png_palette: Default::default(),
        threads: None,
        resize: None,
        crop: None,
        extend: None,
        fill_color: None,
    };

    run_batch(
        &args.input,
        args.output.as_deref(),
        args.recursive,
        args.jobs,
        "convert",
        |file, output| {
            let original_size = std::fs::metadata(file)?.len();
            let (image, _src_format) =
                decode_file(file).with_context(|| format!("{}", file.display()))?;
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
