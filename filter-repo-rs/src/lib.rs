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

pub use opts::Options;
pub use pathutil::dequote_c_style_bytes;

pub fn run(opts: &Options) -> std::io::Result<()> {
  // Optional preflight checks
  crate::sanity::preflight(opts)?;
  if opts.backup {
    if let Some(bundle_path) = crate::backup::create_backup(opts)? {
      println!("Backup bundle saved to {}", bundle_path.display());
    }
  }
  // Sensitive mode: ensure full ref coverage
  crate::migrate::fetch_all_refs_if_needed(opts);
  // Migrate refs/remotes/origin/* -> refs/heads/* for full runs
  crate::migrate::migrate_origin_to_heads(opts)?;
  stream::run(opts)
}
