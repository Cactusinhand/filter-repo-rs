mod message;
pub mod pathutil;
mod gitutil;
mod backup;
pub mod opts;
mod pipes;
mod tag;
mod commit;
mod filechange;
mod finalize;
mod stream;
mod sanity;
mod migrate;
pub mod analysis;

pub use opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};
pub use pathutil::dequote_c_style_bytes;

pub fn run(opts: &Options) -> std::io::Result<()> {
  match opts.mode {
    Mode::Filter => {
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
