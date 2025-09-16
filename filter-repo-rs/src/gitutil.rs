use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn git_dir(repo: &Path) -> io::Result<PathBuf> {
    let out = Command::new("git")
        .arg("-C").arg(repo)
        .arg("rev-parse").arg("--git-dir")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, format!("'git -C {:?} rev-parse --git-dir' failed", repo)));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let p = PathBuf::from(&s);
    if p.is_absolute() {
        Ok(p)
    } else {
        // Make relative .git paths absolute to the repo directory
        Ok(repo.join(p))
    }
}
