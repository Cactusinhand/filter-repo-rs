use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

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

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GitInvocation {
    #[allow(dead_code)]
    pub cwd: String,
    pub args: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug)]
struct GitSpyPaths {
    bin_dir: PathBuf,
    real_git: PathBuf,
}

#[allow(dead_code)]
static GIT_SPY: OnceLock<GitSpyPaths> = OnceLock::new();
#[allow(dead_code)]
static GIT_SPY_LOG_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
fn canonicalize_for_git(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    strip_unc_prefix(&canonical.to_string_lossy())
}

#[cfg(not(windows))]
fn canonicalize_for_git(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(windows)]
fn strip_unc_prefix(path: &str) -> PathBuf {
    const UNC_PREFIX: &str = "\\\\?\\";
    if let Some(stripped) = path.strip_prefix(UNC_PREFIX) {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(path)
    }
}

#[cfg(not(windows))]
fn strip_unc_prefix(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[allow(dead_code)]
pub fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_filter-repo-rs"))
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

#[allow(dead_code)]
fn find_real_git() -> PathBuf {
    let candidate = if cfg!(windows) { "git.exe" } else { "git" };
    let path_env = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path_env) {
        let candidate_path = dir.join(candidate);
        if candidate_path.is_file() {
            return candidate_path;
        }
    }
    panic!("unable to locate real git binary on PATH");
}

#[allow(dead_code)]
fn compile_git_spy(bin_dir: &Path) -> PathBuf {
    let src_path = bin_dir.join("git_spy.rs");
    let bin_name = if cfg!(windows) { "git.exe" } else { "git" };
    let bin_path = bin_dir.join(bin_name);
    let source = r#"
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let log_path = match env::var("FRRS_GIT_LOG") {
        Ok(path) => path,
        Err(_) => std::process::exit(1),
    };
    let real_git = match env::var("FRRS_REAL_GIT") {
        Ok(path) => path,
        Err(_) => std::process::exit(1),
    };
    let cwd = match env::current_dir() {
        Ok(dir) => dir,
        Err(_) => std::process::exit(1),
    };
    let mut fields = Vec::with_capacity(args.len() + 1);
    fields.push(cwd.to_string_lossy().into_owned());
    for arg in &args {
        fields.push(arg.to_string_lossy().into_owned());
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(file, "{}", fields.join("\t"));
    }
    let status = Command::new(real_git).args(&args).status();
    match status {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(_) => std::process::exit(1),
    }
}
"#;
    fs::write(&src_path, source).expect("write git spy source");
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .arg(&src_path)
        .arg("-O")
        .arg("-o")
        .arg(&bin_path)
        .status()
        .expect("invoke rustc for git spy");
    assert!(status.success(), "failed to compile git spy shim");
    bin_path
}

#[allow(dead_code)]
fn ensure_git_spy() -> &'static GitSpyPaths {
    GIT_SPY.get_or_init(|| {
        let bin_dir = mktemp("git_spy");
        fs::create_dir_all(&bin_dir).expect("create git spy dir");
        let real_git = find_real_git();
        let bin_path = compile_git_spy(&bin_dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&bin_path)
                .expect("git spy metadata")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bin_path, perms).expect("set git spy perms");
        }
        #[cfg(not(unix))]
        {
            let _ = &bin_path;
        }
        GitSpyPaths { bin_dir, real_git }
    })
}

#[allow(dead_code)]
fn next_log_path(dir: &Path) -> PathBuf {
    let idx = GIT_SPY_LOG_COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("git-invocations-{}.log", idx))
}

#[allow(dead_code)]
fn parse_git_invocations(log_path: &Path) -> Vec<GitInvocation> {
    let data = match fs::read_to_string(log_path) {
        Ok(data) => data,
        Err(_) => return Vec::new(),
    };
    data.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            let cwd = parts.next().unwrap_or("").to_string();
            let args = parts.map(|s| s.to_string()).collect();
            GitInvocation { cwd, args }
        })
        .collect()
}

#[allow(dead_code)]
pub fn git_commands_for_repo(repo: &Path, invocations: &[GitInvocation]) -> Vec<Vec<String>> {
    let repo_abs_buf = canonicalize_for_git(repo);

    invocations
        .iter()
        .filter_map(|inv| {
            if strip_unc_prefix(&inv.cwd) == repo_abs_buf {
                return Some(inv.args.clone());
            }

            if inv.args.get(0).map(|s| s == "-C").unwrap_or(false)
                && inv
                    .args
                    .get(1)
                    .map(|s| strip_unc_prefix(s) == repo_abs_buf)
                    .unwrap_or(false)
            {
                return Some(inv.args[2..].to_vec());
            }

            None
        })
        .collect()
}

#[allow(dead_code)]
pub fn find_git_command<'a>(commands: &'a [Vec<String>], name: &str) -> Option<&'a Vec<String>> {
    commands
        .iter()
        .find(|cmd| cmd.iter().any(|arg| arg == name))
}

#[allow(dead_code)]
pub fn run_cli_with_git_spy(repo: &Path, extra_args: &[&str]) -> (Output, Vec<GitInvocation>) {
    let spy = ensure_git_spy();
    let log_path = next_log_path(&spy.bin_dir);
    if log_path.exists() {
        let _ = fs::remove_file(&log_path);
    }
    let repo_abs = canonicalize_for_git(repo);
    let repo_str = repo_abs.to_string_lossy().to_string();
    let mut cmd = cli_command();
    let cwd = repo_abs
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_abs.clone());
    cmd.current_dir(cwd);
    cmd.arg("--source").arg(&repo_str);
    cmd.arg("--target").arg(&repo_str);
    for arg in extra_args {
        cmd.arg(arg);
    }
    let current_path = env::var_os("PATH").unwrap_or_default();
    let mut path_entries = vec![spy.bin_dir.clone()];
    path_entries.extend(env::split_paths(&current_path));
    let joined = env::join_paths(path_entries).expect("join PATH for git spy");
    cmd.env("PATH", joined);
    cmd.env("FRRS_REAL_GIT", &spy.real_git);
    cmd.env("FRRS_GIT_LOG", &log_path);
    let output = cmd.output().expect("run filter-repo-rs with git spy");
    let invocations = parse_git_invocations(&log_path);
    (output, invocations)
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
    opts.force = true; // Use --force to bypass sanity checks for unit tests
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
