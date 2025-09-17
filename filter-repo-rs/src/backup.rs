use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
  let bundle_name = format!(
    "{}-{:09}.bundle",
    timestamp.as_secs(),
    timestamp.subsec_nanos(),
  );

  let bundle_path = match &opts.backup_path {
    Some(path) => {
      let resolved = if path.is_absolute() {
        path.clone()
      } else {
        opts.source.join(path)
      };
      if resolved.extension().is_some() {
        if let Some(parent) = resolved.parent() {
          if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
          }
        }
        resolved
      } else {
        fs::create_dir_all(&resolved)?;
        resolved.join(&bundle_name)
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
      io::ErrorKind::Other,
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
