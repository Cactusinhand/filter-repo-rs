use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::gitutil::git_dir;
use crate::opts::Options;

pub fn create_backup(opts: &Options) -> io::Result<Option<PathBuf>> {
  if opts.dry_run { return Ok(None); }

  let git_dir = git_dir(&opts.source).map_err(|e| {
    io::Error::new(
      io::ErrorKind::Other,
      format!("failed to resolve git dir for {:?}: {e}", opts.source),
    )
  })?;

  let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_else(|_| Duration::from_secs(0));
  let nanos_since_epoch = (timestamp.as_secs() as i128)
    .saturating_mul(1_000_000_000)
    + timestamp.subsec_nanos() as i128;
  let datetime = OffsetDateTime::from_unix_timestamp_nanos(nanos_since_epoch)
    .unwrap_or(OffsetDateTime::UNIX_EPOCH);
  const FORMAT: &[FormatItem<'_>] =
    format_description!("[year][month][day]-[hour][minute][second]-[subsecond digits:9]");
  let formatted = datetime.format(FORMAT).map_err(|e| {
    io::Error::new(
      io::ErrorKind::Other,
      format!("failed to format backup timestamp: {e}"),
    )
  })?;
  let bundle_name = format!("backup-{formatted}.bundle");

  let bundle_path = match &opts.backup_path {
    Some(path) => {
      let resolved = if path.is_absolute() {
        path.clone()
      } else {
        opts.source.join(path)
      };
      if resolved.is_dir() || resolved.extension().is_none() {
        fs::create_dir_all(&resolved)?;
        resolved.join(&bundle_name)
      } else {
        if let Some(parent) = resolved.parent() {
          if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
          }
        }
        resolved
      }
    }
    None => {
      let dest = git_dir.join("filter-repo");
      fs::create_dir_all(&dest)?;
      dest.join(&bundle_name)
    }
  };

  if opts.refs.is_empty() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "no refs specified for backup",
    ));
  }

  let status = Command::new("git")
    .arg("-C").arg(&opts.source)
    .arg("bundle").arg("create")
    .arg(&bundle_path)
    .args(opts.refs.iter())
    .status()
    .map_err(|e| {
      io::Error::new(
        io::ErrorKind::Other,
        format!("failed to run git bundle create: {e}"),
      )
    })?;

  if !status.success() {
    return Err(io::Error::new(
      io::ErrorKind::Other,
      format!("git bundle create failed with status {status}"),
    ));
  }

  Ok(Some(bundle_path))
}
