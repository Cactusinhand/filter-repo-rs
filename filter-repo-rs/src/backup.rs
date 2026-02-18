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
    if opts.dry_run {
        return Ok(None);
    }

    let git_dir = git_dir(&opts.source).map_err(|e| {
        io::Error::other(
            format!("failed to resolve git dir for {:?}: {e}", opts.source),
        )
    })?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let nanos_since_epoch = (timestamp.as_secs() as i128).saturating_mul(1_000_000_000)
        + timestamp.subsec_nanos() as i128;
    let datetime = OffsetDateTime::from_unix_timestamp_nanos(nanos_since_epoch)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    const FORMAT: &[FormatItem<'_>] =
        format_description!("[year][month][day]-[hour][minute][second]-[subsecond digits:9]");
    let formatted = datetime.format(FORMAT).map_err(|e| {
        io::Error::other(
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
        .arg("-C")
        .arg(&opts.source)
        .arg("bundle")
        .arg("create")
        .arg(&bundle_path)
        .args(opts.refs.iter())
        .status()
        .map_err(|e| {
            io::Error::other(
                format!("failed to run git bundle create: {e}"),
            )
        })?;

    if !status.success() {
        return Err(io::Error::other(
            format!("git bundle create failed with status {status}"),
        ));
    }

    Ok(Some(bundle_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .expect("git command should run");
        assert!(status.success(), "git command failed: {:?}", args);
    }

    fn init_repo_with_commit() -> TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.name", "Backup Test"]);
        run_git(dir.path(), &["config", "user.email", "backup@test"]);
        std::fs::write(dir.path().join("README.md"), "seed\n").expect("write file");
        run_git(dir.path(), &["add", "README.md"]);
        run_git(dir.path(), &["commit", "-m", "seed"]);
        dir
    }

    #[test]
    fn create_backup_returns_none_for_dry_run() {
        let repo = init_repo_with_commit();
        let opts = Options {
            source: repo.path().to_path_buf(),
            dry_run: true,
            ..Options::default()
        };

        let result = create_backup(&opts).expect("dry-run backup should succeed");
        assert!(result.is_none(), "dry-run should not create bundle");
    }

    #[test]
    fn create_backup_errors_when_refs_are_empty() {
        let repo = init_repo_with_commit();
        let opts = Options {
            source: repo.path().to_path_buf(),
            refs: Vec::new(),
            ..Options::default()
        };

        let err = create_backup(&opts).expect_err("empty refs should fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("no refs specified"));
    }

    #[test]
    fn create_backup_errors_for_non_git_source() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = Options {
            source: dir.path().to_path_buf(),
            ..Options::default()
        };

        let err = create_backup(&opts).expect_err("non-git source should fail");
        assert!(
            err.to_string().contains("failed to resolve git dir"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn create_backup_errors_when_git_bundle_fails() {
        let repo = init_repo_with_commit();
        let opts = Options {
            source: repo.path().to_path_buf(),
            refs: vec!["refs/heads/does-not-exist".to_string()],
            ..Options::default()
        };

        let err = create_backup(&opts).expect_err("invalid refs should fail backup");
        assert!(
            err.to_string().contains("git bundle create failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn create_backup_supports_absolute_directory_override() {
        let repo = init_repo_with_commit();
        let out_dir = tempfile::tempdir().expect("create output dir");
        let target_dir = out_dir.path().join("bundles");
        let opts = Options {
            source: repo.path().to_path_buf(),
            backup_path: Some(target_dir.clone()),
            ..Options::default()
        };

        let bundle = create_backup(&opts)
            .expect("backup should succeed")
            .expect("bundle path should be returned");
        assert!(
            bundle.starts_with(&target_dir),
            "bundle path should be under override directory"
        );
        assert!(bundle.exists(), "bundle should exist: {:?}", bundle);
    }
}
