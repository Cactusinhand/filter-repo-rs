pub mod analysis;
mod backup;
mod commit;
mod filechange;
mod finalize;
mod gitutil;
mod message;
mod migrate;
pub mod opts;
pub mod pathutil;
mod pipes;
mod sanity;
mod stream;
mod tag;

use std::io;

pub use opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};
pub use pathutil::dequote_c_style_bytes;

fn validate_options(opts: &Options) -> io::Result<()> {
    if let Some(max) = opts.max_blob_size {
        if max == 0 || max == usize::MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "max-blob-size must be greater than zero and smaller than usize::MAX",
            ));
        }
    }

    const MAX_PATH_BYTES: usize = 4096;
    for entry in &opts.paths {
        if entry.len() > MAX_PATH_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path filter entries exceed supported length",
            ));
        }
    }

    for (old, new_) in &opts.path_renames {
        if old == new_ {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path rename source and destination must differ",
            ));
        }
        if old.len() > MAX_PATH_BYTES || new_.len() > MAX_PATH_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path rename entries exceed supported length",
            ));
        }
    }

    Ok(())
}

pub fn run(opts: &Options) -> std::io::Result<()> {
    match opts.mode {
        Mode::Filter => {
            validate_options(opts)?;
            crate::sanity::preflight(opts)?;
            if opts.backup {
                if let Some(bundle_path) = crate::backup::create_backup(opts)? {
                    println!("Backup bundle saved to {}", bundle_path.display());
                }
            }
            crate::migrate::fetch_all_refs_if_needed(opts);
            crate::migrate::migrate_origin_to_heads(opts)?;
            stream::run(opts)
        }
        Mode::Analyze => analysis::run(opts),
    }
}
