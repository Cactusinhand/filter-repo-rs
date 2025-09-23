pub mod analysis;
mod backup;
mod commit;
pub mod error;
mod filechange;
mod finalize;
pub mod git_config;
pub mod gitutil;
mod message;
mod migrate;
pub mod opts;
pub mod pathutil;
mod pipes;
pub mod sanity;
mod stream;
mod tag;

pub use self::error::{FilterRepoError, Result as FilterRepoResult};
pub use opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};
pub use pathutil::dequote_c_style_bytes;

fn validate_options(opts: &Options) -> FilterRepoResult<()> {
    if let Some(max) = opts.max_blob_size {
        if max == 0 || max == usize::MAX {
            return Err(FilterRepoError::invalid_options(
                "max-blob-size must be greater than zero and smaller than usize::MAX",
            ));
        }
    }

    const MAX_PATH_BYTES: usize = 4096;
    for entry in &opts.paths {
        if entry.len() > MAX_PATH_BYTES {
            return Err(FilterRepoError::invalid_options(
                "path filter entries exceed supported length",
            ));
        }
    }

    for (old, new_) in &opts.path_renames {
        if old == new_ {
            return Err(FilterRepoError::invalid_options(
                "path rename source and destination must differ",
            ));
        }
        if old.len() > MAX_PATH_BYTES || new_.len() > MAX_PATH_BYTES {
            return Err(FilterRepoError::invalid_options(
                "path rename entries exceed supported length",
            ));
        }
    }

    Ok(())
}

pub fn run(opts: &Options) -> FilterRepoResult<()> {
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
        Mode::Analyze => Ok(analysis::run(opts)?),
    }
}
