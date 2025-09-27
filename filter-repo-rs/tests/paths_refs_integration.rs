use std::path::Path;
use tempfile::TempDir;

use filter_repo_rs::{run, Options};

fn git(args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(args)
        .output()
        .expect("git command failed to spawn")
}

fn git_status_ok(args: &[&str]) {
    let out = git(args);
    assert!(out.status.success(), "git {:?} failed: {:?}", args, out);
}

fn write_file(path: &std::path::Path, data: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, data).unwrap();
}

fn current_head_symref(repo: &Path) -> String {
    let out = git(&["-C", repo.to_str().unwrap(), "symbolic-ref", "-q", "HEAD"]);
    assert!(out.status.success(), "symbolic-ref failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn list_refs(repo: &Path) -> Vec<String> {
    let out = git(&[
        "-C",
        repo.to_str().unwrap(),
        "for-each-ref",
        "--format=%(refname)",
    ]);
    assert!(out.status.success(), "for-each-ref failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .collect()
}

fn ls_tree_paths(repo: &Path, rev: &str) -> Vec<String> {
    let out = git(&["-C", repo.to_str().unwrap(), "ls-tree", "-r", rev]);
    assert!(out.status.success(), "ls-tree failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, p)| p.to_string()))
        .collect()
}

fn commit_all(repo: &Path, msg: &str) {
    git_status_ok(&["-C", repo.to_str().unwrap(), "add", "."]);
    git_status_ok(&["-C", repo.to_str().unwrap(), "commit", "-m", msg]);
}

fn create_src_repo_with_paths() -> (TempDir, String) {
    let src = TempDir::new().unwrap();
    let rp = src.path();

    // Init and configure
    git_status_ok(&["init", rp.to_str().unwrap()]);
    for (k, v) in [
        ("user.name", "Filter Repo Tester"),
        ("user.email", "tester@example.com"),
    ] {
        git_status_ok(&["-C", rp.to_str().unwrap(), "config", k, v]);
    }

    // Commit 1: keep/ and drop/ content
    write_file(&rp.join("keep/one.txt"), b"one");
    write_file(&rp.join("drop/two.txt"), b"two");
    commit_all(rp, "init");

    // Normalize branch name to 'main' for determinism
    git_status_ok(&["-C", rp.to_str().unwrap(), "branch", "-M", "main"]);

    // Commit 2: change only in drop/ so it will be pruned
    write_file(&rp.join("drop/only.txt"), b"pruned");
    commit_all(rp, "drop-only change");

    // Calculate HEAD ref before moving src
    let head_ref = current_head_symref(rp);

    // Return repo and current HEAD ref
    (src, head_ref)
}

fn create_bare_target() -> TempDir {
    let tgt = TempDir::new().unwrap();
    git_status_ok(&["init", "--bare", tgt.path().to_str().unwrap()]);
    tgt
}

fn default_opts(source: &Path, target: &Path) -> Options {
    let mut opts = Options::default();
    opts.source = source.to_path_buf();
    opts.target = target.to_path_buf();
    opts.refs = vec!["--all".to_string()];
    opts.force = true; // bypass preflight in tests
    opts.reset = false; // avoid reset in bare repos
    opts
}

#[test]
fn filter_path_and_branch_rename_updates_head() {
    let (src, head_ref_src) = create_src_repo_with_paths();
    let tgt = create_bare_target();

    let mut opts = default_opts(src.path(), tgt.path());
    // Keep only keep/
    opts.paths.push(b"keep/".to_vec());
    // Rename all branches under a prefix to avoid dependency on default branch name
    opts.branch_rename = Some((Vec::new(), b"filtered/".to_vec()));

    // Run pipeline
    run(&opts).expect("pipeline run");

    // HEAD should move to filtered/<original-branch>
    let expected_head = {
        let tail = head_ref_src
            .strip_prefix("refs/heads/")
            .unwrap_or(&head_ref_src);
        format!("refs/heads/filtered/{}", tail)
    };
    let head_after = current_head_symref(tgt.path());
    assert_eq!(head_after, expected_head);

    // Only keep/ paths should remain in the imported tree
    let paths = ls_tree_paths(tgt.path(), "HEAD");
    assert!(paths.iter().any(|p| p == "keep/one.txt"));
    assert!(paths.iter().all(|p| !p.starts_with("drop/")));

    // The renamed branch exists
    let refs = list_refs(tgt.path());
    assert!(refs.iter().any(|r| r == &expected_head));
}

#[test]
fn commit_map_records_pruned_commits() {
    let (src, _head_ref_src) = create_src_repo_with_paths();
    let tgt = create_bare_target();

    let mut opts = default_opts(src.path(), tgt.path());
    // Keep only keep/ so the second commit (drop-only) is pruned
    opts.paths.push(b"keep/".to_vec());
    // Deterministic import
    opts.quiet = true;

    // Collect original commits
    let rev_out = git(&[
        "-C",
        src.path().to_str().unwrap(),
        "rev-list",
        "--reverse",
        "HEAD",
    ]);
    assert!(rev_out.status.success(), "rev-list failed");
    let mut olds: Vec<String> = String::from_utf8_lossy(&rev_out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .collect();
    assert!(olds.len() >= 2, "need at least two commits");
    let old_first = olds.remove(0); // kept
    let old_second = olds.remove(0); // pruned

    // Run pipeline
    run(&opts).expect("pipeline run");

    // Read commit-map from target debug dir
    let commit_map_path = tgt.path().join("filter-repo").join("commit-map");
    let data = std::fs::read_to_string(commit_map_path).expect("commit-map");

    // Expect first commit mapped to a real id (not zero), and second to zeros
    let mut got_first = false;
    let mut got_second = false;
    for line in data.lines() {
        let mut it = line.split_whitespace();
        if let (Some(old), Some(new_)) = (it.next(), it.next()) {
            if old == old_first {
                assert_ne!(new_, "0000000000000000000000000000000000000000");
                got_first = true;
            }
            if old == old_second {
                assert_eq!(new_, "0000000000000000000000000000000000000000");
                got_second = true;
            }
        }
    }
    assert!(got_first, "first commit mapping missing");
    assert!(got_second, "pruned commit mapping missing");
}
