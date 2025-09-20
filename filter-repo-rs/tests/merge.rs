use std::fs;
use std::process::{Command, Stdio};

mod common;
use common::*;

#[test]
fn merge_parents_dedup_when_side_branch_pruned() {
    let repo = init_repo();
    let base_branch = current_branch(&repo);

    write_file(&repo, "keep.txt", "base");
    assert_eq!(run_git(&repo, &["add", "keep.txt"]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "add keep path"]).0, 0);

    assert_eq!(run_git(&repo, &["checkout", "-b", "feature-branch"]).0, 0);
    write_file(&repo, "drop.txt", "side change");
    assert_eq!(run_git(&repo, &["add", "drop.txt"]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "side branch change"]).0, 0);

    assert_eq!(run_git(&repo, &["checkout", &base_branch]).0, 0);
    assert_eq!(
        run_git(
            &repo,
            &["merge", "--no-ff", "--no-commit", "feature-branch"]
        )
        .0,
        0,
        "expected merge to succeed",
    );
    write_file(&repo, "keep.txt", "merge resolution");
    assert_eq!(run_git(&repo, &["add", "keep.txt"]).0, 0);
    assert_eq!(
        run_git(&repo, &["commit", "-m", "merge feature branch"]).0,
        0
    );

    run_tool_expect_success(&repo, |opts| {
        opts.paths.push(b"keep.txt".to_vec());
    });

    let filtered_path = repo
        .join(".git")
        .join("filter-repo")
        .join("fast-export.filtered");

    let import_repo = mktemp("fr_rs_merge_import");
    fs::create_dir_all(&import_repo).expect("create import repo directory");
    assert_eq!(run_git(&import_repo, &["init"]).0, 0, "git init failed");

    let stream = fs::File::open(&filtered_path).expect("open filtered fast-export stream");
    let status = Command::new("git")
        .current_dir(&import_repo)
        .arg("fast-import")
        .stdin(Stdio::from(stream))
        .status()
        .expect("run git fast-import");
    assert!(status.success(), "git fast-import failed: {status:?}");

    let (code, parents, stderr) = run_git(
        &import_repo,
        &["log", "--grep=merge feature branch", "-1", "--pretty=%P"],
    );
    assert_eq!(code, 0, "git log failed: {}", stderr);
    let parents: Vec<_> = parents
        .split_whitespace()
        .filter(|p| !p.is_empty())
        .collect();
    assert_eq!(
        parents.len(),
        1,
        "expected single parent after filtering, got: {:?}",
        parents
    );
}
