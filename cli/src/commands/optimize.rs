use std::path::PathBuf;

use clap::Args;
use slimg_core::{EncodeOptions, optimize_with_options, output_path};

use super::{FileOutcome, run_batch, safe_write};

#[derive(Debug, Args)]
pub struct OptimizeArgs {
    /// Input file or directory
    pub input: PathBuf,

    /// Encoding quality (0-100)
    #[arg(short, long, default_value_t = 80)]
    pub quality: u8,

    /// Encoding effort (0-100, higher is slower/smaller)
    #[arg(short, long)]
    pub effort: Option<u8>,

    /// Output path (file or directory)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Overwrite existing files (required for in-place optimization)
    #[arg(long)]
    pub overwrite: bool,

    /// Process subdirectories recursively
    #[arg(long)]
    pub recursive: bool,

    /// Number of parallel jobs (defaults to CPU count)
    #[arg(short, long)]
    pub jobs: Option<usize>,
}

pub fn run(args: OptimizeArgs) -> anyhow::Result<()> {
    // Without --output, optimize writes back to the input file; require the
    // explicit opt-in before touching anything.
    if args.output.is_none() && !args.overwrite {
        anyhow::bail!(
            "optimize replaces the input file in place; pass --overwrite to \
             confirm, or use --output to write elsewhere"
        );
    }

    let has_explicit_output = args.output.is_some();

    run_batch(
        &args.input,
        args.output.as_deref(),
        args.recursive,
        args.jobs,
        "optimize",
        |file, output| {
            let original_data = std::fs::read(file)?;
            let original_size = original_data.len() as u64;

            let result = optimize_with_options(
                &original_data,
                EncodeOptions {
                    quality: args.quality,
                    effort: args.effort,
                    png_palette: Default::default(),
                    threads: None,
                },
            )?;
            let new_size = result.data.len() as u64;

            let out = match output {
                Some(out) => output_path(file, result.format, Some(out)),
                // In-place: write back to the exact input path.
                None => file.to_path_buf(),
            };

            // Writing a same-or-larger file over the original is pointless;
            // with an explicit --output the caller expects a file either way.
            if new_size >= original_size && !has_explicit_output {
                return Ok(FileOutcome::Skipped {
                    reason: format!("optimized size {new_size} >= original {original_size}"),
                });
            }

            safe_write(&out, &result.data, args.overwrite)?;

            Ok(FileOutcome::Written {
                out,
                original_size,
                new_size,
            })
        },
    )
}
