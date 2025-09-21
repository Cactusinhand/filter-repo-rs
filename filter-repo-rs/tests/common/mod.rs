use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use filter_repo_rs as fr;

pub fn mktemp(prefix: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push("it");
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pid = std::process::id();
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let c = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("{}_{}_{}_{}", prefix, pid, t, c));
    p
}

pub fn run_git(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    (code, stdout, stderr)
}

pub fn write_file(dir: &Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    let mut f = File::create(&path).unwrap();
    f.write_all(contents.as_bytes()).unwrap();
}

pub fn init_repo() -> PathBuf {
    let repo = mktemp("fr_rs_it");
    fs::create_dir_all(&repo).unwrap();
    let (c, _o, e) = run_git(&repo, &["init"]);
    assert_eq!(c, 0, "git init failed: {}", e);
    assert_eq!(
        run_git(&repo, &["config", "user.name", "A U Thor"]).0,
        0,
        "failed to set user.name"
    );
    assert_eq!(
        run_git(&repo, &["config", "user.email", "a.u.thor@example.com"]).0,
        0,
        "failed to set user.email"
    );
    write_file(&repo, "README.md", "hello");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0, "git add failed");
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "init commit"]).0, 0);
    repo
}

#[allow(dead_code)]
pub fn run_tool(dir: &Path, configure: impl FnOnce(&mut fr::Options)) -> std::io::Result<()> {
    let mut opts = fr::Options::default();
    opts.source = dir.to_path_buf();
    opts.target = dir.to_path_buf();
    configure(&mut opts);
    fr::run(&opts)
}

#[allow(dead_code)]
pub fn run_tool_expect_success(dir: &Path, configure: impl FnOnce(&mut fr::Options)) {
    run_tool(dir, configure).expect("filter-repo-rs run should succeed");
}

#[allow(dead_code)]
pub fn current_branch(repo: &Path) -> String {
    let (_, branch, _) = run_git(repo, &["symbolic-ref", "--short", "HEAD"]);
    let mut branch = branch.trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        let (_, alt, _) = run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        branch = alt.trim().to_string();
    }
    branch
}

#[allow(dead_code)]
pub fn docs_example_config_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("docs");
    path.push("examples");
    path.push("filter-repo-rs.toml");
    path
}
