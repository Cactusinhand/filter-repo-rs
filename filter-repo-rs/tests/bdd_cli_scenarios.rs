use std::fs;

use filter_repo_rs as fr;

mod common;
use common::*;

fn stage_and_commit(repo: &std::path::Path, message: &str) {
    assert_eq!(run_git(repo, &["add", "."]).0, 0, "git add failed");
    assert_eq!(
        run_git(repo, &["commit", "-q", "-m", message]).0,
        0,
        "git commit failed"
    );
}

fn list_tree(repo: &std::path::Path) -> String {
    let (_code, out, _err) = run_git(
        repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    out
}

#[test]
fn given_secret_blob_when_replace_text_then_redacted_value_is_persisted() {
    let repo = init_repo();
    write_file(&repo, "secrets.env", "API_KEY=SECRET-ABC-123\n");
    stage_and_commit(&repo, "add secrets file");

    let rules = repo.join("replace-text-rules.txt");
    fs::write(&rules, "SECRET-ABC-123==>REDACTED\n").expect("write replace-text rules");

    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(rules.clone());
        o.no_data = false;
    });

    let (_code, content, _err) = run_git(&repo, &["show", "HEAD:secrets.env"]);
    assert!(content.contains("REDACTED"));
    assert!(!content.contains("SECRET-ABC-123"));
}

#[test]
fn given_commit_and_tag_messages_when_replace_message_then_tokens_are_rewritten() {
    let repo = init_repo();
    write_file(&repo, "src/lib.rs", "pub fn f() {}\n");
    stage_and_commit(&repo, "commit with FOO token");
    assert_eq!(
        run_git(
            &repo,
            &["tag", "-a", "-m", "tag includes FOO token", "v1.0"]
        )
        .0,
        0
    );

    let rules = repo.join("replace-message-rules.txt");
    fs::write(&rules, "FOO==>BAR\n").expect("write replace-message rules");

    run_tool_expect_success(&repo, |o| {
        o.replace_message_file = Some(rules.clone());
        o.no_data = true;
    });

    let (_c1, msg, _e1) = run_git(&repo, &["log", "-1", "--format=%B"]);
    assert!(msg.contains("BAR"));
    assert!(!msg.contains("FOO"));

    let (_c2, tag_oid, _e2) = run_git(&repo, &["rev-parse", "refs/tags/v1.0"]);
    let (_c3, tag_obj, _e3) = run_git(&repo, &["cat-file", "-p", tag_oid.trim()]);
    assert!(tag_obj.contains("BAR"));
    assert!(!tag_obj.contains("FOO"));
}

#[test]
fn given_mixed_blob_sizes_when_max_blob_size_is_set_then_large_blobs_are_removed() {
    let repo = init_repo();
    fs::write(repo.join("small.txt"), "small\n").expect("write small file");
    fs::write(repo.join("large.bin"), vec![b'X'; 4096]).expect("write large file");
    stage_and_commit(&repo, "add mixed-size blobs");

    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1024);
        o.no_data = false;
    });

    let tree = list_tree(&repo);
    assert!(
        tree.contains("small.txt"),
        "expected small file kept: {}",
        tree
    );
    assert!(
        !tree.contains("large.bin"),
        "expected large file removed: {}",
        tree
    );
}

#[test]
fn given_subdirectory_filter_when_run_then_selected_subdir_becomes_repository_root() {
    let repo = init_repo();
    write_file(&repo, "frontend/app.js", "console.log('frontend');\n");
    write_file(&repo, "backend/api.rs", "fn main() {}\n");
    stage_and_commit(&repo, "add frontend and backend");

    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"frontend/".to_vec());
        o.path_renames.push((b"frontend/".to_vec(), Vec::new()));
    });

    let tree = list_tree(&repo);
    assert!(
        tree.contains("app.js"),
        "expected frontend file at root: {}",
        tree
    );
    assert!(
        !tree.contains("frontend/app.js"),
        "expected original frontend prefix removed: {}",
        tree
    );
    assert!(
        !tree.contains("backend/api.rs"),
        "expected backend removed by filter: {}",
        tree
    );
}

#[test]
fn given_to_subdirectory_filter_when_run_then_history_is_moved_under_new_prefix() {
    let repo = init_repo();
    write_file(&repo, "README.data", "hello\n");
    stage_and_commit(&repo, "add top-level file");

    run_tool_expect_success(&repo, |o| {
        o.path_renames
            .push((Vec::new(), b"packages/core/".to_vec()));
    });

    let tree = list_tree(&repo);
    assert!(
        tree.contains("packages/core/README.data"),
        "expected file moved under prefix: {}",
        tree
    );
}

#[test]
fn given_invert_paths_when_run_then_matching_paths_are_removed() {
    let repo = init_repo();
    write_file(&repo, "keep/ok.txt", "ok\n");
    write_file(&repo, "docs/secret.md", "secret\n");
    stage_and_commit(&repo, "seed invert-path test");

    run_tool_expect_success(&repo, |o| {
        o.paths.push(b"docs/".to_vec());
        o.invert_paths = true;
    });

    let tree = list_tree(&repo);
    assert!(
        tree.contains("keep/ok.txt"),
        "expected keep file present: {}",
        tree
    );
    assert!(
        !tree.contains("docs/secret.md"),
        "expected docs path removed: {}",
        tree
    );
}

#[test]
fn given_branch_and_tag_rename_rules_when_run_then_refs_are_renamed() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["checkout", "-b", "feature/topic"]).0, 0);
    write_file(&repo, "src/feature.txt", "feature\n");
    stage_and_commit(&repo, "feature branch commit");
    assert_eq!(
        run_git(&repo, &["tag", "-a", "-m", "release tag", "v1.2"]).0,
        0
    );

    run_tool_expect_success(&repo, |o| {
        o.branch_rename = Some((b"feature/".to_vec(), b"prod/".to_vec()));
        o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    });

    assert_eq!(
        run_git(&repo, &["show-ref", "--verify", "refs/heads/prod/topic"]).0,
        0
    );
    assert_ne!(
        run_git(&repo, &["show-ref", "--verify", "refs/heads/feature/topic"]).0,
        0
    );
    assert_eq!(
        run_git(&repo, &["show-ref", "--verify", "refs/tags/release-1.2"]).0,
        0
    );
    assert_ne!(
        run_git(&repo, &["show-ref", "--verify", "refs/tags/v1.2"]).0,
        0
    );
    let (_c, head_ref, _e) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    assert_eq!(head_ref.trim(), "refs/heads/prod/topic");
}

#[test]
fn given_backup_enabled_when_run_then_bundle_artifact_is_created() {
    let repo = init_repo();
    run_tool_expect_success(&repo, |o| {
        o.backup = true;
        o.no_data = true;
    });
    let backup_dir = repo.join(".git").join("filter-repo");
    assert!(
        backup_dir.exists(),
        "backup dir should exist: {:?}",
        backup_dir
    );
    let has_bundle = fs::read_dir(&backup_dir)
        .expect("read backup dir")
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("bundle"));
    assert!(has_bundle, "expected at least one backup bundle");
}

#[test]
fn given_dry_run_mode_when_run_then_head_and_origin_remote_are_unchanged() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    let (_c0, head_before, _e0) = run_git(&repo, &["rev-parse", "HEAD"]);

    run_tool_expect_success(&repo, |o| {
        o.dry_run = true;
        o.write_report = true;
        o.no_data = true;
    });

    let (_c1, head_after, _e1) = run_git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(head_before.trim(), head_after.trim());
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
}

#[test]
fn given_sensitive_mode_when_run_then_origin_remote_is_preserved() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);

    run_tool_expect_success(&repo, |o| {
        o.sensitive = true;
        o.no_fetch = true;
        o.no_data = true;
    });

    let (_c, remotes, _e) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
}

#[test]
fn given_nonsensitive_full_rewrite_when_run_then_origin_remote_is_removed() {
    let repo = init_repo();
    let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let headref = headref.trim().to_string();
    let branch = headref
        .strip_prefix("refs/heads/")
        .unwrap_or(&headref)
        .to_string();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    let spec = format!("+{}:refs/remotes/origin/{}", headref, branch);
    assert_eq!(run_git(&repo, &["fetch", "origin", &spec]).0, 0);
    assert_eq!(
        run_git(
            &repo,
            &[
                "show-ref",
                "--verify",
                &format!("refs/remotes/origin/{}", branch)
            ]
        )
        .0,
        0
    );

    run_tool_expect_success(&repo, |_o| {});

    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(!remotes.contains("origin"));
}

#[test]
fn given_analyze_mode_when_run_then_metrics_and_warnings_are_reported() {
    let repo = init_repo();
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.mode = fr::Mode::Analyze;
    opts.force = true;

    let report = fr::analysis::generate_report(&opts).expect("generate analyze report");
    assert!(report.metrics.refs_total >= 1, "expected refs_total >= 1");
    assert!(
        !report.warnings.is_empty(),
        "expected non-empty warnings/info entries"
    );
    fr::run(&opts).expect("analyze mode should run successfully");
}

#[test]
fn given_write_report_enabled_when_run_then_report_and_map_artifacts_are_emitted() {
    let repo = init_repo();
    write_file(&repo, "docs/report-input.txt", "hello\n");
    stage_and_commit(&repo, "seed report artifacts test");

    run_tool_expect_success(&repo, |o| {
        o.write_report = true;
        o.no_data = true;
    });

    let debug_dir = repo.join(".git").join("filter-repo");
    assert!(debug_dir.join("report.txt").exists());
    assert!(debug_dir.join("commit-map").exists());
    assert!(debug_dir.join("fast-export.filtered").exists());
}
