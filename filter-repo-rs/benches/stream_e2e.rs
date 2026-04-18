//! End-to-end integration benchmark for the stream processing pipeline.
//!
//! Creates a synthetic git repository, then measures the full filter-repo
//! pipeline (fast-export → process → fast-import) under various configurations.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use criterion::{criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

#[path = "../tests/common/fake_secrets.rs"]
mod fake_secrets;

use filter_repo_rs::opts::{Mode, Options};

// ---------------------------------------------------------------------------
// Fixture: a reusable synthetic repository
// ---------------------------------------------------------------------------

struct Fixture {
    /// Bare repo that serves as the template (never modified by benchmarks).
    bare_dir: TempDir,
}

/// Returns a shared fixture, built once across all benchmarks.
fn fixture() -> &'static Fixture {
    static INSTANCE: OnceLock<Fixture> = OnceLock::new();
    INSTANCE.get_or_init(|| build_fixture(200, 6))
}

/// Create a bare repository with `n_commits` commits, each touching `files_per`
/// files spread across several directories.  Content is deterministic so SHA
/// stability is guaranteed across runs.
fn build_fixture(n_commits: usize, files_per: usize) -> Fixture {
    let work = TempDir::new().expect("create workdir");
    let work_path = work.path();

    // git init
    run_git(work_path, &["init", "-b", "main"]);
    run_git(work_path, &["config", "user.email", "bench@test.local"]);
    run_git(work_path, &["config", "user.name", "Bench User"]);

    // Directories to spread files across
    let dirs = ["src", "lib", "tests", "docs", "config", "scripts"];
    for d in &dirs {
        fs::create_dir_all(work_path.join(d)).unwrap();
    }

    // Create replace-text rules file (for content-replacement benchmarks)
    let rules_file = work_path.join("bench_rules.txt");
    {
        let secret_value_alpha = fake_secrets::secret_value_alpha();
        let private_token_beta = fake_secrets::private_token_beta();
        let internal_key_gamma = fake_secrets::internal_key_gamma();
        let mut f = fs::File::create(&rules_file).unwrap();
        writeln!(
            f,
            "{}",
            fake_secrets::replace_rule(&secret_value_alpha, "***REMOVED***")
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            fake_secrets::replace_rule(&private_token_beta, "***REMOVED***")
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            fake_secrets::replace_rule(&internal_key_gamma, "***REMOVED***")
        )
        .unwrap();
    }

    let secret_value_alpha = fake_secrets::secret_value_alpha();
    let private_token_beta = fake_secrets::private_token_beta();

    for commit_idx in 0..n_commits {
        for file_idx in 0..files_per {
            let dir = dirs[(commit_idx + file_idx) % dirs.len()];
            let filename = format!("{}/file_{}.txt", dir, file_idx);
            let filepath = work_path.join(&filename);

            // Deterministic content; some lines contain replaceable tokens
            let mut content = String::with_capacity(2048);
            for line in 0..40 {
                if line == 10 && commit_idx % 3 == 0 {
                    content.push_str(&format!("  api_key = {secret_value_alpha}\n"));
                } else if line == 20 && commit_idx % 5 == 0 {
                    content.push_str(&format!("  token = {private_token_beta}\n"));
                } else {
                    content.push_str(&format!(
                        "line {} of commit {} in {}/{}\n",
                        line, commit_idx, dir, file_idx
                    ));
                }
            }
            fs::write(&filepath, content).unwrap();
        }
        run_git(work_path, &["add", "-A"]);
        let msg = format!("commit {}", commit_idx);
        run_git(work_path, &["commit", "-m", &msg, "--allow-empty"]);
    }

    // Clone to bare (this is the template we clone from per-iteration)
    let bare = TempDir::new().expect("create bare dir");
    run_git(
        work_path,
        &["clone", "--bare", ".", bare.path().to_str().unwrap()],
    );

    // Copy the rules file into bare dir for later use
    fs::copy(&rules_file, bare.path().join("bench_rules.txt")).unwrap();

    Fixture { bare_dir: bare }
}

/// Clone the fixture bare repo into a fresh temporary directory (non-bare).
fn clone_fixture(fixture: &Fixture) -> TempDir {
    let dest = TempDir::new().expect("create clone dir");
    // Clone bare → working copy
    Command::new("git")
        .args([
            "clone",
            fixture.bare_dir.path().to_str().unwrap(),
            dest.path().to_str().unwrap(),
        ])
        .output()
        .expect("git clone");
    dest
}

fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git command failed");
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        panic!("git {:?} failed in {}: {}", args, dir.display(), stderr);
    }
}

fn make_opts(repo: &Path) -> Options {
    Options {
        source: repo.to_path_buf(),
        target: repo.to_path_buf(),
        force: true,
        enforce_sanity: false,
        quiet: true,
        mode: Mode::Filter,
        cleanup: filter_repo_rs::opts::CleanupMode::None,
        backup: false,
        reset: false,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_stream_passthrough(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 1: passthrough (no filters) — pure I/O + parsing overhead
    group.bench_function("passthrough", |b| {
        b.iter_with_setup(
            || clone_fixture(fix),
            |clone_dir| {
                let opts = make_opts(clone_dir.path());
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    group.finish();
}

fn bench_stream_path_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 2: path filter — keep only src/
    group.bench_function("path_filter_src", |b| {
        b.iter_with_setup(
            || clone_fixture(fix),
            |clone_dir| {
                let mut opts = make_opts(clone_dir.path());
                opts.paths = vec![b"src/".to_vec()];
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    // Scenario 3: glob filter — keep *.txt in any directory
    group.bench_function("glob_filter_txt", |b| {
        b.iter_with_setup(
            || clone_fixture(fix),
            |clone_dir| {
                let mut opts = make_opts(clone_dir.path());
                opts.path_globs = vec![b"**/*.txt".to_vec()];
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    group.finish();
}

fn bench_stream_content_replace(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 4: content replacement (--replace-text)
    group.bench_function("content_replace", |b| {
        b.iter_with_setup(
            || {
                let clone_dir = clone_fixture(fix);
                // Copy rules file into the cloned repo
                let rules_src = fix.bare_dir.path().join("bench_rules.txt");
                let rules_dst = clone_dir.path().join("bench_rules.txt");
                fs::copy(&rules_src, &rules_dst).unwrap();
                clone_dir
            },
            |clone_dir| {
                let mut opts = make_opts(clone_dir.path());
                opts.replace_text_file = Some(clone_dir.path().join("bench_rules.txt"));
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    group.finish();
}

fn bench_stream_combined(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 5: combined (path filter + content replacement + path rename)
    group.bench_function("combined", |b| {
        b.iter_with_setup(
            || {
                let clone_dir = clone_fixture(fix);
                let rules_src = fix.bare_dir.path().join("bench_rules.txt");
                let rules_dst = clone_dir.path().join("bench_rules.txt");
                fs::copy(&rules_src, &rules_dst).unwrap();
                clone_dir
            },
            |clone_dir| {
                let mut opts = make_opts(clone_dir.path());
                opts.paths = vec![b"src/".to_vec(), b"lib/".to_vec()];
                opts.path_renames = vec![(b"lib/".to_vec(), b"packages/lib/".to_vec())];
                opts.replace_text_file = Some(clone_dir.path().join("bench_rules.txt"));
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    group.finish();
}

fn bench_stream_max_blob_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 6: --max-blob-size (triggers BlobSizeTracker prefetch + filtering)
    group.bench_function("max_blob_size", |b| {
        b.iter_with_setup(
            || clone_fixture(fix),
            |clone_dir| {
                let mut opts = make_opts(clone_dir.path());
                opts.max_blob_size = Some(512); // small limit to trigger filtering
                filter_repo_rs::run(&opts).expect("filter run");
            },
        );
    });

    group.finish();
}

fn bench_analyze(c: &mut Criterion) {
    let mut group = c.benchmark_group("stream_e2e");
    group.sample_size(10);
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(15));

    let fix = fixture();

    // Scenario 7: --analyze mode (read-only analysis, no rewriting)
    group.bench_function("analyze", |b| {
        b.iter_with_setup(
            || clone_fixture(fix),
            |clone_dir| {
                let opts = Options {
                    source: clone_dir.path().to_path_buf(),
                    target: clone_dir.path().to_path_buf(),
                    mode: Mode::Analyze,
                    quiet: true,
                    ..Default::default()
                };
                filter_repo_rs::run(&opts).expect("analyze run");
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_stream_passthrough,
    bench_stream_path_filter,
    bench_stream_content_replace,
    bench_stream_combined,
    bench_stream_max_blob_size,
    bench_analyze
);
criterion_main!(benches);
