pub mod convert;
pub mod crop;
pub mod extend;
pub mod optimize;
pub mod resize;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use clap::ValueEnum;
use indicatif::{ProgressBar, ProgressStyle};
use slimg_core::Format;

/// Image format argument for CLI.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FormatArg {
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
    Qoi,
}

impl FormatArg {
    pub fn into_format(self) -> Format {
        match self {
            Self::Jpeg => Format::Jpeg,
            Self::Png => Format::Png,
            Self::Webp => Format::WebP,
            Self::Avif => Format::Avif,
            Self::Jxl => Format::Jxl,
            Self::Qoi => Format::Qoi,
        }
    }
}

/// Known image file extensions that slimg can process.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "avif", "jxl", "qoi"];

/// Collect image files from a path.
///
/// - If `path` is a file, returns it as a single-element vec.
/// - If `path` is a directory, lists entries (recursively if `recursive` is true),
///   filters by image extensions, and returns sorted results.
pub(crate) fn collect_files(path: &Path, recursive: bool) -> anyhow::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    if !path.is_dir() {
        anyhow::bail!("{} is not a file or directory", path.display());
    }

    let mut files = Vec::new();
    collect_dir(path, recursive, &mut files)?;
    files.sort();
    Ok(files)
}

/// Configure rayon's global thread pool.
/// When `jobs` is `None`, rayon defaults to the number of logical CPUs.
pub(crate) fn configure_thread_pool(jobs: Option<usize>) -> anyhow::Result<()> {
    if let Some(n) = jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .map_err(|e| anyhow::anyhow!("failed to configure thread pool: {e}"))?;
    }
    Ok(())
}

/// Create a progress bar for batch processing.
/// Returns a hidden bar when processing a single file.
pub(crate) fn make_progress_bar(total: usize) -> ProgressBar {
    if total <= 1 {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:30}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("=> "),
    );
    pb
}

/// Write data to a file atomically (temp file + rename), so a crash mid-write
/// never leaves a corrupt or truncated file behind.
///
/// Refuses to replace an existing file unless `overwrite` is set.
pub(crate) fn safe_write(path: &Path, data: &[u8], overwrite: bool) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !overwrite && path.exists() {
        anyhow::bail!(
            "{} already exists (pass --overwrite to replace it)",
            path.display()
        );
    }

    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".slimg_tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }

    Ok(())
}

/// Validate and prepare the `--output` argument for a run.
///
/// When processing multiple files (or a directory input), `--output` must be
/// a directory; it is created if it does not exist yet. Otherwise every
/// worker would race to write the same single file.
pub(crate) fn resolve_output(
    input: &Path,
    output: Option<&Path>,
    file_count: usize,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(out) = output else {
        return Ok(None);
    };

    let is_batch = file_count > 1 || input.is_dir();
    if is_batch {
        if out.exists() && !out.is_dir() {
            anyhow::bail!(
                "--output must be a directory when processing multiple files, \
                 but {} is a file",
                out.display()
            );
        }
        fs::create_dir_all(out)?;
    }

    Ok(Some(out.to_path_buf()))
}

/// Result of processing a single file inside a batch.
pub(crate) enum FileOutcome {
    Written {
        out: PathBuf,
        original_size: u64,
        new_size: u64,
    },
    Skipped {
        reason: String,
    },
}

/// Shared batch driver used by every subcommand: collects input files,
/// validates the output target, runs `process` for each file on the rayon
/// pool, reports per-file results and a failure summary, and returns an
/// error if any file failed.
pub(crate) fn run_batch<F>(
    input: &Path,
    output: Option<&Path>,
    recursive: bool,
    jobs: Option<usize>,
    verb: &str,
    process: F,
) -> anyhow::Result<()>
where
    F: Fn(&Path, Option<&Path>) -> anyhow::Result<FileOutcome> + Sync,
{
    use rayon::prelude::*;

    let files = collect_files(input, recursive)?;
    if files.is_empty() {
        anyhow::bail!("no image files found in {}", input.display());
    }

    let output = resolve_output(input, output, files.len())?;
    configure_thread_pool(jobs)?;

    let pb = make_progress_bar(files.len());
    let errors = ErrorCollector::new();

    files.par_iter().for_each(|file| {
        match process(file, output.as_deref()) {
            Ok(FileOutcome::Written {
                out,
                original_size,
                new_size,
            }) => {
                let ratio = if original_size > 0 {
                    (new_size as f64 / original_size as f64) * 100.0
                } else {
                    0.0
                };
                pb.println(format!(
                    "{} -> {} ({} -> {} bytes, {:.1}%)",
                    file.display(),
                    out.display(),
                    original_size,
                    new_size,
                    ratio,
                ));
            }
            Ok(FileOutcome::Skipped { reason }) => {
                pb.println(format!("{} -> skipped ({reason})", file.display()));
            }
            Err(e) => errors.push(file, &e),
        }
        pb.inc(1);
    });

    let fail_count = errors.summarize(&pb);
    pb.finish_and_clear();

    if fail_count > 0 {
        anyhow::bail!("{fail_count} file(s) failed to {verb}");
    }

    Ok(())
}

/// Collector for errors that occur during batch processing.
/// Thread-safe — can be shared across rayon workers.
pub(crate) struct ErrorCollector {
    errors: Mutex<Vec<(PathBuf, String)>>,
}

impl ErrorCollector {
    pub fn new() -> Self {
        Self {
            errors: Mutex::new(Vec::new()),
        }
    }

    pub fn push(&self, path: &Path, err: &anyhow::Error) {
        self.errors
            .lock()
            .unwrap()
            .push((path.to_path_buf(), format!("{err:#}")));
    }

    /// Print error summary and return the error count.
    pub fn summarize(&self, pb: &ProgressBar) -> usize {
        let errors = self.errors.lock().unwrap();
        if errors.is_empty() {
            return 0;
        }

        pb.println(format!("\n{} file(s) failed:", errors.len()));
        for (path, msg) in errors.iter() {
            pb.println(format!("  {} — {}", path.display(), msg));
        }

        errors.len()
    }
}

/// Parse "WxH" size string (e.g. "1920x1080").
pub(crate) fn parse_size(s: &str) -> std::result::Result<(u32, u32), String> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err("expected format: WIDTHxHEIGHT (e.g. 1920x1080)".to_string());
    }
    let w: u32 = parts[0]
        .trim()
        .parse()
        .map_err(|_| format!("invalid width: '{}'", parts[0]))?;
    let h: u32 = parts[1]
        .trim()
        .parse()
        .map_err(|_| format!("invalid height: '{}'", parts[1]))?;
    if w == 0 || h == 0 {
        return Err("size values must be non-zero".to_string());
    }
    Ok((w, h))
}

fn collect_dir(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // `file_type()` does not follow symlinks: symlinked directories are
        // never recursed into (avoids symlink cycles), while symlinks to
        // regular files are still processed.
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if recursive {
                collect_dir(&path, recursive, out)?;
            }
        } else if (file_type.is_file() || path.is_file())
            && let Some(ext) = path.extension().and_then(|e| e.to_str())
            && IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── safe_write ──────────────────────────────────────────

    #[test]
    fn safe_write_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.bin");

        safe_write(&path, b"hello", false).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn safe_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a/b/c/out.bin");

        safe_write(&path, b"nested", false).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"nested");
    }

    #[test]
    fn safe_write_overwrite_replaces_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file.jpg");
        fs::write(&path, b"original").unwrap();

        safe_write(&path, b"optimized", true).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"optimized");
    }

    #[test]
    fn safe_write_overwrite_leaves_no_temp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file.jpg");
        fs::write(&path, b"original").unwrap();

        safe_write(&path, b"new", true).unwrap();

        let tmp = dir.path().join("file.jpg.slimg_tmp");
        assert!(!tmp.exists(), "temp file should be cleaned up after rename");
    }

    #[test]
    fn safe_write_overwrite_no_collision_different_extensions() {
        let dir = TempDir::new().unwrap();
        let jpg = dir.path().join("photo.jpg");
        let png = dir.path().join("photo.png");
        fs::write(&jpg, b"jpg-original").unwrap();
        fs::write(&png, b"png-original").unwrap();

        // Simulate parallel overwrite of both files
        safe_write(&jpg, b"jpg-new", true).unwrap();
        safe_write(&png, b"png-new", true).unwrap();

        assert_eq!(fs::read(&jpg).unwrap(), b"jpg-new");
        assert_eq!(fs::read(&png).unwrap(), b"png-new");

        // Verify distinct temp paths
        let jpg_tmp = dir.path().join("photo.jpg.slimg_tmp");
        let png_tmp = dir.path().join("photo.png.slimg_tmp");
        assert!(!jpg_tmp.exists());
        assert!(!png_tmp.exists());
    }

    #[test]
    fn safe_write_overwrite_preserves_original_on_nonexistent_target() {
        // When the file doesn't exist yet, overwrite=true still creates it
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new.jpg");

        safe_write(&path, b"data", true).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"data");
    }

    #[test]
    fn safe_write_refuses_existing_without_overwrite() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("file.jpg");
        fs::write(&path, b"original").unwrap();

        let result = safe_write(&path, b"new", false);

        assert!(result.is_err(), "must refuse to clobber without overwrite");
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[test]
    fn safe_write_new_file_is_atomic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.bin");

        safe_write(&path, b"data", false).unwrap();

        let tmp = dir.path().join("out.bin.slimg_tmp");
        assert!(!tmp.exists(), "temp file should be cleaned up after rename");
    }

    // ── resolve_output ──────────────────────────────────────

    #[test]
    fn resolve_output_none_passthrough() {
        let dir = TempDir::new().unwrap();
        assert_eq!(resolve_output(dir.path(), None, 5).unwrap(), None);
    }

    #[test]
    fn resolve_output_batch_rejects_file_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("out.webp");
        fs::write(&target, b"").unwrap();

        let result = resolve_output(dir.path(), Some(&target), 3);
        assert!(
            result.is_err(),
            "batch output onto a single file must be rejected"
        );
    }

    #[test]
    fn resolve_output_batch_creates_missing_directory() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("newdir");

        let resolved = resolve_output(dir.path(), Some(&target), 3).unwrap();

        assert_eq!(resolved, Some(target.clone()));
        assert!(
            target.is_dir(),
            "missing output directory should be created"
        );
    }

    #[test]
    fn resolve_output_single_file_keeps_file_target() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("in.jpg");
        fs::write(&input, b"").unwrap();
        let target = dir.path().join("out.webp");

        let resolved = resolve_output(&input, Some(&target), 1).unwrap();

        assert_eq!(resolved, Some(target.clone()));
        assert!(!target.exists(), "single-file output must not become a dir");
    }

    // ── ErrorCollector ──────────────────────────────────────

    #[test]
    fn error_collector_empty_returns_zero() {
        let ec = ErrorCollector::new();
        let pb = ProgressBar::hidden();

        assert_eq!(ec.summarize(&pb), 0);
    }

    #[test]
    fn error_collector_counts_errors() {
        let ec = ErrorCollector::new();
        ec.push(Path::new("a.jpg"), &anyhow::anyhow!("decode failed"));
        ec.push(Path::new("b.png"), &anyhow::anyhow!("io error"));

        let pb = ProgressBar::hidden();
        assert_eq!(ec.summarize(&pb), 2);
    }

    #[test]
    fn error_collector_is_thread_safe() {
        use rayon::prelude::*;

        let ec = ErrorCollector::new();
        let paths: Vec<PathBuf> = (0..100)
            .map(|i| PathBuf::from(format!("{i}.jpg")))
            .collect();

        paths.par_iter().for_each(|p| {
            ec.push(p, &anyhow::anyhow!("error"));
        });

        let pb = ProgressBar::hidden();
        assert_eq!(ec.summarize(&pb), 100);
    }

    // ── collect_files ───────────────────────────────────────

    #[test]
    fn collect_files_single_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.jpg");
        fs::write(&path, b"fake").unwrap();

        let files = collect_files(&path, false).unwrap();
        assert_eq!(files, vec![path]);
    }

    #[test]
    fn collect_files_filters_by_extension() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.jpg"), b"").unwrap();
        fs::write(dir.path().join("b.txt"), b"").unwrap();
        fs::write(dir.path().join("c.png"), b"").unwrap();

        let files = collect_files(dir.path(), false).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| {
            let ext = f.extension().unwrap().to_str().unwrap();
            ext == "jpg" || ext == "png"
        }));
    }

    #[test]
    fn collect_files_recursive() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.jpg"), b"").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.png"), b"").unwrap();

        let non_recursive = collect_files(dir.path(), false).unwrap();
        assert_eq!(non_recursive.len(), 1);

        let recursive = collect_files(dir.path(), true).unwrap();
        assert_eq!(recursive.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn collect_files_does_not_follow_symlinked_dirs() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.jpg"), b"").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("b.png"), b"").unwrap();
        // Symlink cycle: sub/loop -> parent dir. Following it would recurse
        // forever.
        std::os::unix::fs::symlink(dir.path(), sub.join("loop")).unwrap();

        let files = collect_files(dir.path(), true).unwrap();

        assert_eq!(files.len(), 2, "symlinked dirs must not be recursed into");
    }

    #[cfg(unix)]
    #[test]
    fn collect_files_includes_symlinked_files() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real.jpg");
        fs::write(&real, b"").unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("link.jpg")).unwrap();

        let files = collect_files(dir.path(), false).unwrap();

        assert_eq!(files.len(), 2, "symlinks to regular files are processed");
    }

    // ── parse_size ────────────────────────────────────────────

    #[test]
    fn parse_size_valid() {
        assert_eq!(parse_size("1920x1080"), Ok((1920, 1080)));
    }

    #[test]
    fn parse_size_with_spaces() {
        assert_eq!(parse_size("1920 x 1080"), Ok((1920, 1080)));
    }

    #[test]
    fn parse_size_zero() {
        assert!(parse_size("0x100").is_err());
    }

    #[test]
    fn parse_size_wrong_format() {
        assert!(parse_size("1920-1080").is_err());
    }

    #[test]
    fn parse_size_invalid_number() {
        assert!(parse_size("abcx100").is_err());
    }
}
