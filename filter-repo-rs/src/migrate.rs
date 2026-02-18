use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::git_config::GitConfig;
use crate::gitutil;
use crate::opts::Options;

#[allow(dead_code)]
pub fn fetch_all_refs_if_needed(opts: &Options) -> io::Result<()> {
    if !opts.sensitive || opts.no_fetch || opts.dry_run {
        return Ok(());
    }
    // Check that origin exists
    let remotes = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("remote")
        .output()
        .map_err(|e| {
            io::Error::other(
                format!("failed to run git remote: {e}"),
            )
        })?;
    if !remotes.status.success() {
        eprintln!("WARNING: --sensitive: git remote command failed, skipping ref fetch");
        return Ok(());
    }
    let r = String::from_utf8_lossy(&remotes.stdout);
    if !r.lines().any(|l| l.trim() == "origin") {
        eprintln!("WARNING: --sensitive: no 'origin' remote found, skipping ref fetch");
        return Ok(());
    }
    // Fetch all refs to ensure sensitive-history coverage
    eprintln!("NOTICE: Fetching all refs from origin to ensure full sensitive-history coverage");
    let status = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("fetch")
        .arg("-q")
        .arg("--prune")
        .arg("--update-head-ok")
        .arg("--refmap")
        .arg("")
        .arg("origin")
        .arg("+refs/*:refs/*")
        .status()
        .map_err(|e| {
            io::Error::other(
                format!("failed to run git fetch: {e}"),
            )
        })?;
    if !status.success() {
        return Err(io::Error::other(
            "git fetch command failed with non-zero exit status",
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn migrate_origin_to_heads(opts: &Options) -> io::Result<()> {
    if opts.partial || opts.dry_run {
        return Ok(());
    }
    // List refs under refs/remotes/origin/*
    let refs = match gitutil::get_all_refs(&opts.source) {
        Ok(refs) => refs,
        Err(_) => return Ok(()),
    };
    let mut to_create: Vec<(String, String)> = Vec::new();
    let mut to_delete: Vec<(String, String)> = Vec::new();
    for (refname, hash) in refs
        .iter()
        .filter(|(name, _)| name.starts_with("refs/remotes/origin/"))
    {
        let hash = hash.clone();
        if refname == "refs/remotes/origin/HEAD" {
            to_delete.push((refname.clone(), hash));
            continue;
        }
        let suffix = refname
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(refname);
        let newref = format!("refs/heads/{}", suffix);
        // Only create if newref does not exist
        let exist = refs.contains_key(&newref);
        if !exist {
            to_create.push((newref, hash.clone()));
        }
        to_delete.push((refname.clone(), hash));
    }
    if to_create.is_empty() && to_delete.is_empty() {
        return Ok(());
    }
    // Batch update-ref
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("update-ref")
        .arg("--no-deref")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        for (r, h) in to_create.iter() {
            writeln!(stdin, "create {} {}", r, h).map_err(|e| {
                io::Error::other(
                    format!("failed to write to git update-ref stdin: {e}"),
                )
            })?;
        }
        for (r, h) in to_delete.iter() {
            writeln!(stdin, "delete {} {}", r, h).map_err(|e| {
                io::Error::other(
                    format!("failed to write to git update-ref stdin: {e}"),
                )
            })?;
        }
    }
    let status = child.wait().map_err(|e| {
        io::Error::other(
            format!("failed to wait for git update-ref: {e}"),
        )
    })?;
    if !status.success() {
        return Err(io::Error::other(
            "git update-ref command failed with non-zero exit status",
        ));
    }
    Ok(())
}

pub fn remove_origin_remote_if_applicable(opts: &Options) -> io::Result<()> {
    if opts.sensitive || opts.partial || opts.dry_run {
        return Ok(());
    }
    // Check that origin exists
    let remotes = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("remote")
        .output()
        .map_err(|e| {
            io::Error::other(
                format!("failed to run git remote: {e}"),
            )
        })?;
    if !remotes.status.success() {
        return Ok(());
    }
    let r = String::from_utf8_lossy(&remotes.stdout);
    if !r.lines().any(|l| l.trim() == "origin") {
        return Ok(());
    }
    // Print URL for context if available
    let url = GitConfig::get_string_config(&opts.target, "remote.origin.url")
        .ok()
        .and_then(|value| value)
        .unwrap_or_default();
    if url.is_empty() {
        eprintln!("NOTICE: Removing 'origin' remote; see docs if you want to push back there.");
    } else {
        eprintln!("NOTICE: Removing 'origin' remote (was: {})", url);
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("remote")
        .arg("rm")
        .arg("origin")
        .status()
        .map_err(|e| {
            io::Error::other(
                format!("failed to run git remote rm: {e}"),
            )
        })?;
    if !status.success() {
        return Err(io::Error::other(
            "git remote rm command failed with non-zero exit status",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_status(repo: &std::path::Path, args: &[&str]) -> std::process::ExitStatus {
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .expect("git command should execute")
    }

    fn git_output(repo: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git command should execute");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    }

    fn init_repo_with_commit() -> TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        assert!(git_status(dir.path(), &["init"]).success());
        assert!(git_status(dir.path(), &["config", "user.name", "Migrate Test"]).success());
        assert!(git_status(dir.path(), &["config", "user.email", "migrate@test"]).success());
        std::fs::write(dir.path().join("README.md"), "seed\n").expect("write README");
        assert!(git_status(dir.path(), &["add", "README.md"]).success());
        assert!(git_status(dir.path(), &["commit", "-m", "seed"]).success());
        dir
    }

    #[test]
    fn fetch_all_refs_returns_early_when_disabled() {
        let repo = init_repo_with_commit();

        let plain = Options {
            source: repo.path().to_path_buf(),
            sensitive: false,
            ..Options::default()
        };
        assert!(fetch_all_refs_if_needed(&plain).is_ok());

        let no_fetch = Options {
            source: repo.path().to_path_buf(),
            sensitive: true,
            no_fetch: true,
            ..Options::default()
        };
        assert!(fetch_all_refs_if_needed(&no_fetch).is_ok());

        let dry = Options {
            source: repo.path().to_path_buf(),
            sensitive: true,
            dry_run: true,
            ..Options::default()
        };
        assert!(fetch_all_refs_if_needed(&dry).is_ok());
    }

    #[test]
    fn fetch_all_refs_skips_when_origin_missing() {
        let repo = init_repo_with_commit();
        let opts = Options {
            source: repo.path().to_path_buf(),
            sensitive: true,
            ..Options::default()
        };

        assert!(
            fetch_all_refs_if_needed(&opts).is_ok(),
            "missing origin should be treated as non-fatal"
        );
    }

    #[test]
    fn fetch_all_refs_returns_error_when_fetch_fails() {
        let repo = init_repo_with_commit();
        assert!(git_status(
            repo.path(),
            &["remote", "add", "origin", "/path/that/does/not/exist"]
        )
        .success());
        let opts = Options {
            source: repo.path().to_path_buf(),
            sensitive: true,
            ..Options::default()
        };

        let err = fetch_all_refs_if_needed(&opts).expect_err("fetch should fail");
        assert!(
            err.to_string().contains("non-zero exit status"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn migrate_origin_to_heads_moves_remote_tracking_refs() {
        let repo = init_repo_with_commit();
        let (_code, head, _err) = git_output(repo.path(), &["rev-parse", "HEAD"]);
        let head = head.trim();
        assert!(git_status(
            repo.path(),
            &["update-ref", "refs/remotes/origin/feature", head]
        )
        .success());
        assert!(git_status(
            repo.path(),
            &["update-ref", "refs/remotes/origin/HEAD", head]
        )
        .success());

        let opts = Options {
            source: repo.path().to_path_buf(),
            ..Options::default()
        };
        migrate_origin_to_heads(&opts).expect("migration should succeed");

        let (feature_code, _, _) = git_output(repo.path(), &["show-ref", "refs/heads/feature"]);
        assert_eq!(feature_code, 0, "expected refs/heads/feature to be created");

        let (remote_code, _, _) =
            git_output(repo.path(), &["show-ref", "refs/remotes/origin/feature"]);
        assert_ne!(remote_code, 0, "remote-tracking ref should be removed");
        let (head_code, _, _) = git_output(repo.path(), &["show-ref", "refs/remotes/origin/HEAD"]);
        assert_ne!(head_code, 0, "origin/HEAD should be removed");
    }

    #[test]
    fn migrate_origin_to_heads_returns_ok_when_source_is_not_git_repo() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let opts = Options {
            source: dir.path().to_path_buf(),
            ..Options::default()
        };
        assert!(migrate_origin_to_heads(&opts).is_ok());
    }

    #[test]
    fn remove_origin_remote_respects_guard_flags() {
        let repo = init_repo_with_commit();
        assert!(git_status(repo.path(), &["remote", "add", "origin", "."]).success());

        let sensitive = Options {
            target: repo.path().to_path_buf(),
            sensitive: true,
            ..Options::default()
        };
        remove_origin_remote_if_applicable(&sensitive).expect("sensitive mode should skip");
        let (_, remotes, _) = git_output(repo.path(), &["remote"]);
        assert!(remotes.lines().any(|line| line.trim() == "origin"));

        let partial = Options {
            target: repo.path().to_path_buf(),
            partial: true,
            ..Options::default()
        };
        remove_origin_remote_if_applicable(&partial).expect("partial mode should skip");

        let dry = Options {
            target: repo.path().to_path_buf(),
            dry_run: true,
            ..Options::default()
        };
        remove_origin_remote_if_applicable(&dry).expect("dry-run should skip");
    }

    #[test]
    fn remove_origin_remote_removes_existing_origin() {
        let repo = init_repo_with_commit();
        assert!(git_status(repo.path(), &["remote", "add", "origin", "."]).success());

        let opts = Options {
            target: repo.path().to_path_buf(),
            ..Options::default()
        };
        remove_origin_remote_if_applicable(&opts).expect("remove origin should succeed");
        let (_, remotes, _) = git_output(repo.path(), &["remote"]);
        assert!(
            !remotes.lines().any(|line| line.trim() == "origin"),
            "origin should be removed"
        );
    }

    #[test]
    fn remove_origin_remote_returns_error_on_rm_failure() {
        let repo = init_repo_with_commit();
        assert!(git_status(repo.path(), &["remote", "add", "origin", "."]).success());
        std::fs::create_dir(repo.path().join(".git").join("config.lock"))
            .expect("create directory to block git config lockfile");

        let opts = Options {
            target: repo.path().to_path_buf(),
            ..Options::default()
        };
        let err = remove_origin_remote_if_applicable(&opts).expect_err("rm failure should error");
        assert!(
            err.to_string().contains("non-zero exit status"),
            "unexpected error: {err}"
        );
    }
}
