use std::process::Command;

use crate::opts::Options;

fn run(cmd: &mut Command) -> Option<String> {
  cmd.output().ok().and_then(|o| if o.status.success() {
    Some(String::from_utf8_lossy(&o.stdout).to_string())
  } else { None })
}

pub fn preflight(opts: &Options) -> std::io::Result<()> {
  if opts.force { return Ok(()); }
  // Only enforce when requested
  if !opts.enforce_sanity { return Ok(()); }

  let dir = &opts.target;

  // 1) count-objects -v: accept freshly packed (<=1 pack) or no packs with <100 loose
  if let Some(out) = run(Command::new("git").arg("-C").arg(dir).arg("count-objects").arg("-v")) {
    let mut packs = 0usize; let mut count = 0usize;
    for line in out.lines() {
      if let Some(v) = line.strip_prefix("packs: ") { packs = v.trim().parse().unwrap_or(0); }
      if let Some(v) = line.strip_prefix("count: ") { count = v.trim().parse().unwrap_or(0); }
    }
    let freshly_packed = (packs <= 1) && (packs == 0 || count == 0) || (packs == 0 && count < 100);
    if !freshly_packed {
      return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: expected freshly packed repo (use --force to bypass)"));
    }
  }

  // 2) remotes: allow 'origin' or no remotes when no packs
  let remotes = run(Command::new("git").arg("-C").arg(dir).arg("remote")).unwrap_or_default();
  let remote_trim = remotes.trim();
  if !(remote_trim == "origin" || remote_trim.is_empty()) {
    return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: expected one remote 'origin' or no remotes"));
  }

  // 3) stash must be empty
  let stash_present = Command::new("git")
    .arg("-C")
    .arg(dir)
    .arg("rev-parse")
    .arg("--verify")
    .arg("--quiet")
    .arg("refs/stash")
    .status()
    .ok()
    .map(|s| s.success())
    .unwrap_or(false);
  if stash_present {
    return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: stashed changes present"));
  }

  // 4) no staged/unstaged changes
  let staged_dirty = Command::new("git").arg("-C").arg(dir).arg("diff").arg("--staged").arg("--quiet").status().ok().map(|s| !s.success()).unwrap_or(false);
  let dirty = Command::new("git").arg("-C").arg(dir).arg("diff").arg("--quiet").status().ok().map(|s| !s.success()).unwrap_or(false);
  if staged_dirty || dirty {
    return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: working tree not clean"));
  }

  // Determine whether the repository is bare; we only flag working tree
  // cleanliness issues for non-bare repositories.
  let is_bare = run(&mut Command::new("git")
      .arg("-C")
      .arg(dir)
      .arg("rev-parse")
      .arg("--is-bare-repository"))
      .map_or(false, |s| s.trim().eq_ignore_ascii_case("true"));

  if !is_bare {
    // 5) no untracked (ignore the interpreter-generated __pycache__ artifacts
    //     created when running git-filter-repo itself)
    if let Some(out) = run(&mut Command::new("git").arg("-C").arg(dir).arg("ls-files").arg("-o")) {
      if out.lines().any(|line| {
          let l = line.trim();
          !l.is_empty() && !l.starts_with("__pycache__/git_filter_repo.")
      }) {
          return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: untracked files present"));
      }
  }
  }

  // 6) single worktree
  if let Some(out) = run(Command::new("git").arg("-C").arg(dir).arg("worktree").arg("list")) {
    if out.lines().count() > 1 {
      return Err(std::io::Error::new(std::io::ErrorKind::Other, "sanity: multiple worktrees found"));
    }
  }

  Ok(())
}

