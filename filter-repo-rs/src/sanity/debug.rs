use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use super::{SanityCheckContext, SanityCheckError};

#[derive(Debug)]
pub enum GitCommandError {
    NotFound,
    ExecutionFailed {
        command: String,
        stderr: String,
        exit_code: i32,
    },
    Timeout {
        command: String,
        timeout: Duration,
    },
    IoError(String),
    RetryExhausted {
        command: String,
        attempts: u32,
        last_error: Box<GitCommandError>,
    },
}

impl Clone for GitCommandError {
    fn clone(&self) -> Self {
        match self {
            GitCommandError::NotFound => GitCommandError::NotFound,
            GitCommandError::ExecutionFailed {
                command,
                stderr,
                exit_code,
            } => GitCommandError::ExecutionFailed {
                command: command.clone(),
                stderr: stderr.clone(),
                exit_code: *exit_code,
            },
            GitCommandError::Timeout { command, timeout } => GitCommandError::Timeout {
                command: command.clone(),
                timeout: *timeout,
            },
            GitCommandError::IoError(msg) => GitCommandError::IoError(msg.clone()),
            GitCommandError::RetryExhausted {
                command,
                attempts,
                last_error,
            } => GitCommandError::RetryExhausted {
                command: command.clone(),
                attempts: *attempts,
                last_error: last_error.clone(),
            },
        }
    }
}

impl fmt::Display for GitCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitCommandError::NotFound => {
                writeln!(f, "Git executable not found on PATH.")?;
                writeln!(
                    f,
                    "Please install Git and ensure it's available in your PATH."
                )?;
                write!(
                    f,
                    "Visit https://git-scm.com/downloads for installation instructions."
                )
            }
            GitCommandError::ExecutionFailed {
                command,
                stderr,
                exit_code,
            } => {
                writeln!(f, "Git command failed: {}", command)?;
                writeln!(f, "Exit code: {}", exit_code)?;
                if !stderr.is_empty() {
                    write!(f, "Error output: {}", stderr)
                } else {
                    write!(f, "No error output available.")
                }
            }
            GitCommandError::Timeout { command, timeout } => {
                writeln!(f, "Git command timed out after {:?}: {}", timeout, command)?;
                writeln!(f, "The operation may be taking longer than expected.")?;
                write!(
                    f,
                    "Consider checking your repository size or network connectivity."
                )
            }
            GitCommandError::IoError(msg) => {
                write!(f, "IO error during Git command execution: {}", msg)
            }
            GitCommandError::RetryExhausted {
                command,
                attempts,
                last_error,
            } => {
                writeln!(
                    f,
                    "Git command failed after {} attempts: {}",
                    attempts, command
                )?;
                write!(f, "Last error: {}", last_error)
            }
        }
    }
}

impl std::error::Error for GitCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GitCommandError::RetryExhausted { last_error, .. } => Some(last_error.as_ref()),
            _ => None,
        }
    }
}

impl From<io::Error> for GitCommandError {
    fn from(err: io::Error) -> Self {
        GitCommandError::IoError(err.to_string())
    }
}

pub struct DebugOutputManager {
    enabled: bool,
    start_time: Instant,
}

impl DebugOutputManager {
    pub fn new(debug_enabled: bool) -> Self {
        DebugOutputManager {
            enabled: debug_enabled,
            start_time: Instant::now(),
        }
    }

    pub fn log_context_creation(&self, context: &SanityCheckContext) {
        if !self.enabled {
            return;
        }
        let elapsed = self.start_time.elapsed();
        println!(
            "[DEBUG] [{:>8.2}ms] Context created for repository: {}",
            elapsed.as_secs_f64() * 1000.0,
            context.repo_path.display()
        );
        println!(
            "[DEBUG] [{:>8.2}ms]   Repository type: {}",
            elapsed.as_secs_f64() * 1000.0,
            if context.is_bare { "bare" } else { "non-bare" }
        );
        println!(
            "[DEBUG] [{:>8.2}ms]   References found: {}",
            elapsed.as_secs_f64() * 1000.0,
            context.refs.len()
        );
        if !context.replace_refs.is_empty() {
            println!(
                "[DEBUG] [{:>8.2}ms]   Replace references: {}",
                elapsed.as_secs_f64() * 1000.0,
                context.replace_refs.len()
            );
        }
        println!(
            "[DEBUG] [{:>8.2}ms]   Case-insensitive filesystem: {}",
            elapsed.as_secs_f64() * 1000.0,
            context.config.ignore_case
        );
        if context.config.precompose_unicode {
            println!(
                "[DEBUG] [{:>8.2}ms]   Unicode precomposition enabled",
                elapsed.as_secs_f64() * 1000.0
            );
        }
        if let Some(ref remote_url) = context.config.origin_url {
            println!(
                "[DEBUG] [{:>8.2}ms]   Remote origin URL: {}",
                elapsed.as_secs_f64() * 1000.0,
                remote_url
            );
        }
    }

    pub fn log_git_command(
        &self,
        args: &[&str],
        duration: Duration,
        result: &Result<String, GitCommandError>,
    ) {
        if !self.enabled {
            return;
        }
        let elapsed = self.start_time.elapsed();
        let command_str = format!("git {}", args.join(" "));
        match result {
            Ok(output) => {
                let output_preview = if output.len() > 100 {
                    format!("{}... ({} chars)", &output[..97], output.len())
                } else {
                    output.clone()
                };
                println!(
                    "[DEBUG] [{:>8.2}ms] Git command succeeded in {:>6.2}ms: {}",
                    elapsed.as_secs_f64() * 1000.0,
                    duration.as_secs_f64() * 1000.0,
                    command_str
                );
                if !output.trim().is_empty() {
                    println!(
                        "[DEBUG] [{:>8.2}ms]   Output: {}",
                        elapsed.as_secs_f64() * 1000.0,
                        output_preview
                    );
                }
            }
            Err(e) => {
                println!(
                    "[DEBUG] [{:>8.2}ms] Git command failed in {:>6.2}ms: {}",
                    elapsed.as_secs_f64() * 1000.0,
                    duration.as_secs_f64() * 1000.0,
                    command_str
                );
                println!(
                    "[DEBUG] [{:>8.2}ms]   Error: {}",
                    elapsed.as_secs_f64() * 1000.0,
                    e
                );
            }
        }
    }

    pub fn log_sanity_check(&self, check_name: &str, result: &Result<(), SanityCheckError>) {
        if !self.enabled {
            return;
        }
        let elapsed = self.start_time.elapsed();
        match result {
            Ok(()) => {
                println!(
                    "[DEBUG] [{:>8.2}ms] Sanity check PASSED: {}",
                    elapsed.as_secs_f64() * 1000.0,
                    check_name
                );
                match check_name {
                    "git_dir_structure" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Git directory structure is valid",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "reference_conflicts" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: No reference name conflicts detected",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "reflog_entries" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Repository appears fresh (acceptable reflog entries)",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "unpushed_changes" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: All local branches match their remote counterparts",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "freshly_packed" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Repository is freshly packed with acceptable object count",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "remote_configuration" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Remote configuration is valid",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "stash_presence" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: No stashed changes found",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "working_tree_cleanliness" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Working tree is clean",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "untracked_files" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: No untracked files found",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "worktree_count" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Single worktree detected",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "already_ran_detection" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Already ran detection completed successfully",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    "sensitive_mode_validation" => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Sensitive mode options are compatible",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                    _ => println!(
                        "[DEBUG] [{:>8.2}ms]   Reason: Check completed successfully",
                        elapsed.as_secs_f64() * 1000.0
                    ),
                }
            }
            Err(e) => {
                println!(
                    "[DEBUG] [{:>8.2}ms] Sanity check FAILED: {}",
                    elapsed.as_secs_f64() * 1000.0,
                    check_name
                );
                match e {
                    SanityCheckError::GitDirStructure {
                        expected,
                        actual,
                        is_bare,
                    } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Git directory structure mismatch",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Expected: {}, Found: {}, Bare: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            expected,
                            actual,
                            is_bare
                        );
                    }
                    SanityCheckError::ReferenceConflict {
                        conflict_type,
                        conflicts,
                    } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Reference name conflicts detected",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Conflict type: {:?}, Count: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            conflict_type,
                            conflicts.len()
                        );
                    }
                    SanityCheckError::ReflogTooManyEntries {
                        problematic_reflogs,
                    } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Repository not fresh (too many reflog entries)",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Problematic reflogs: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            problematic_reflogs.len()
                        );
                    }
                    SanityCheckError::UnpushedChanges { unpushed_branches } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Unpushed changes detected",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Unpushed branches: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            unpushed_branches.len()
                        );
                    }
                    SanityCheckError::NotFreshlyPacked {
                        packs,
                        loose_count,
                        replace_refs_count,
                    } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Repository not freshly packed",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Packs: {}, Loose objects: {}, Replace refs: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            packs,
                            loose_count,
                            replace_refs_count
                        );
                    }
                    SanityCheckError::WorkingTreeNotClean {
                        staged_dirty,
                        unstaged_dirty,
                    } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Working tree not clean",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Staged dirty: {}, Unstaged dirty: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            staged_dirty,
                            unstaged_dirty
                        );
                    }
                    SanityCheckError::UntrackedFiles { files } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Untracked files present",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Untracked file count: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            files.len()
                        );
                    }
                    SanityCheckError::AlreadyRan { age_hours, .. } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Already ran detection triggered",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Age: {} hours",
                            elapsed.as_secs_f64() * 1000.0,
                            age_hours
                        );
                    }
                    SanityCheckError::SensitiveDataIncompatible { option, .. } => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: Sensitive mode incompatibility",
                            elapsed.as_secs_f64() * 1000.0
                        );
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Incompatible option: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            option
                        );
                    }
                    _ => {
                        println!(
                            "[DEBUG] [{:>8.2}ms]   Reason: {}",
                            elapsed.as_secs_f64() * 1000.0,
                            e
                        );
                    }
                }
            }
        }
    }

    pub fn log_preflight_summary(&self, total_duration: Duration, checks_performed: usize) {
        if !self.enabled {
            return;
        }
        let elapsed = self.start_time.elapsed();
        println!(
            "[DEBUG] [{:>8.2}ms] ========================================",
            elapsed.as_secs_f64() * 1000.0
        );
        println!(
            "[DEBUG] [{:>8.2}ms] Preflight checks completed",
            elapsed.as_secs_f64() * 1000.0
        );
        println!(
            "[DEBUG] [{:>8.2}ms]   Total duration: {:>6.2}ms",
            elapsed.as_secs_f64() * 1000.0,
            total_duration.as_secs_f64() * 1000.0
        );
        println!(
            "[DEBUG] [{:>8.2}ms]   Checks performed: {}",
            elapsed.as_secs_f64() * 1000.0,
            checks_performed
        );
        if checks_performed > 0 {
            let avg_duration = total_duration.as_secs_f64() * 1000.0 / checks_performed as f64;
            println!(
                "[DEBUG] [{:>8.2}ms]   Average check duration: {:>6.2}ms",
                elapsed.as_secs_f64() * 1000.0,
                avg_duration
            );
        }
        let total_ms = total_duration.as_secs_f64() * 1000.0;
        if total_ms > 100.0 {
            println!(
                "[DEBUG] [{:>8.2}ms]   Performance: SLOW (>{:.0}ms threshold)",
                elapsed.as_secs_f64() * 1000.0,
                100.0
            );
        } else if total_ms > 50.0 {
            println!(
                "[DEBUG] [{:>8.2}ms]   Performance: MODERATE (>{:.0}ms threshold)",
                elapsed.as_secs_f64() * 1000.0,
                50.0
            );
        } else {
            println!(
                "[DEBUG] [{:>8.2}ms]   Performance: FAST (<{:.0}ms threshold)",
                elapsed.as_secs_f64() * 1000.0,
                50.0
            );
        }
        println!(
            "[DEBUG] [{:>8.2}ms] ========================================",
            elapsed.as_secs_f64() * 1000.0
        );
    }

    pub fn log_message(&self, message: &str) {
        if !self.enabled {
            return;
        }
        let elapsed = self.start_time.elapsed();
        println!(
            "[DEBUG] [{:>8.2}ms] {}",
            elapsed.as_secs_f64() * 1000.0,
            message
        );
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

pub struct GitCommandExecutor {
    repo_path: PathBuf,
    default_timeout: Duration,
    default_retry_count: u32,
}

impl GitCommandExecutor {
    pub fn new(repo_path: &Path) -> Self {
        GitCommandExecutor {
            repo_path: repo_path.to_path_buf(),
            default_timeout: Duration::from_secs(30),
            default_retry_count: 3,
        }
    }

    pub fn with_settings(repo_path: &Path, timeout: Duration, retry_count: u32) -> Self {
        GitCommandExecutor {
            repo_path: repo_path.to_path_buf(),
            default_timeout: timeout,
            default_retry_count: retry_count,
        }
    }

    pub fn run_command(&self, args: &[&str]) -> Result<String, GitCommandError> {
        self.run_command_with_timeout(args, self.default_timeout)
    }

    pub fn run_command_with_timeout(
        &self,
        args: &[&str],
        timeout: Duration,
    ) -> Result<String, GitCommandError> {
        self.check_git_availability()?;
        let command_str = format!("git -C {} {}", self.repo_path.display(), args.join(" "));
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.repo_path);
        for arg in args {
            cmd.arg(arg);
        }
        let start_time = Instant::now();
        let result = self.execute_with_timeout(cmd, timeout);
        match result {
            Ok(output) => {
                if output.status.success() {
                    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let exit_code = output.status.code().unwrap_or(-1);
                    Err(GitCommandError::ExecutionFailed {
                        command: command_str,
                        stderr,
                        exit_code,
                    })
                }
            }
            Err(e) => {
                if start_time.elapsed() >= timeout {
                    Err(GitCommandError::Timeout {
                        command: command_str,
                        timeout,
                    })
                } else {
                    Err(GitCommandError::IoError(e.to_string()))
                }
            }
        }
    }

    pub fn run_command_with_retry(
        &self,
        args: &[&str],
        max_retries: u32,
    ) -> Result<String, GitCommandError> {
        let mut last_error = None;
        let mut backoff_ms = 100;
        for attempt in 1..=max_retries {
            match self.run_command_with_timeout(args, self.default_timeout) {
                Ok(output) => return Ok(output),
                Err(e) => {
                    last_error = Some(e);
                    if let Some(ref err) = last_error {
                        match err {
                            GitCommandError::NotFound => break,
                            GitCommandError::ExecutionFailed { exit_code, .. } => {
                                if *exit_code == 128 || *exit_code == 129 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    if attempt < max_retries {
                        thread::sleep(Duration::from_millis(backoff_ms));
                        backoff_ms = (backoff_ms * 2).min(5000);
                    }
                }
            }
        }
        let command_str = format!("git -C {} {}", self.repo_path.display(), args.join(" "));
        Err(GitCommandError::RetryExhausted {
            command: command_str,
            attempts: max_retries,
            last_error: Box::new(last_error.unwrap()),
        })
    }

    pub fn run_command_with_default_retry(&self, args: &[&str]) -> Result<String, GitCommandError> {
        self.run_command_with_retry(args, self.default_retry_count)
    }

    pub fn check_git_availability(&self) -> Result<(), GitCommandError> {
        match Command::new("git").arg("--version").output() {
            Ok(output) => {
                if output.status.success() {
                    Ok(())
                } else {
                    Err(GitCommandError::NotFound)
                }
            }
            Err(_) => Err(GitCommandError::NotFound),
        }
    }

    fn execute_with_timeout(
        &self,
        mut cmd: Command,
        timeout: Duration,
    ) -> Result<std::process::Output, io::Error> {
        use std::process::Stdio;
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let start_time = Instant::now();
        loop {
            match child.try_wait()? {
                Some(status) => {
                    let stdout = {
                        let mut buf = Vec::new();
                        if let Some(mut stdout) = child.stdout.take() {
                            use std::io::Read;
                            stdout.read_to_end(&mut buf)?;
                        }
                        buf
                    };
                    let stderr = {
                        let mut buf = Vec::new();
                        if let Some(mut stderr) = child.stderr.take() {
                            use std::io::Read;
                            stderr.read_to_end(&mut buf)?;
                        }
                        buf
                    };
                    return Ok(std::process::Output {
                        status,
                        stdout,
                        stderr,
                    });
                }
                None => {
                    if start_time.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "Command timed out"));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}
