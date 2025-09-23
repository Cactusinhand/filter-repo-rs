mod common;
use common::*;

#[test]
fn dry_run_does_not_modify_refs_or_remote() {
    let repo = init_repo();
    let (_c0, head_before, _e0) = run_git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    run_tool_expect_success(&repo, |o| {
        o.dry_run = true;
        o.write_report = true;
        o.no_data = true;
    });
    let (_c1, head_after, _e1) = run_git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(head_before.trim(), head_after.trim());
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
    let report = repo.join(".git").join("filter-repo").join("report.txt");
    assert!(report.exists());
}

#[test]
fn partial_mode_keeps_origin_and_remote_tracking() {
    let repo = init_repo();
    let (_c, headref, _e) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let headref = headref.trim().to_string();
    let branch = headref
        .strip_prefix("refs/heads/")
        .unwrap_or(&headref)
        .to_string();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    let spec = format!("+{}:refs/remotes/origin/{}", headref, branch);
    assert_eq!(run_git(&repo, &["fetch", "origin", &spec]).0, 0);
    run_tool_expect_success(&repo, |o| {
        o.partial = true;
    });
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
    let (_c3, out, _e3) = run_git(
        &repo,
        &[
            "show-ref",
            "--verify",
            &format!("refs/remotes/origin/{}", branch),
        ],
    );
    assert!(!out.is_empty());
}

#[test]
fn sensitive_fetch_all_from_bare_remote() {
    // Create bare
    let bare = mktemp("fr_rs_bare");
    std::fs::create_dir_all(&bare).unwrap();
    assert_eq!(run_git(&bare, &["init", "--bare"]).0, 0);

    // Seed repo
    let seed = init_repo();
    assert_eq!(run_git(&seed, &["checkout", "-b", "extra"]).0, 0);
    write_file(&seed, "extra.txt", "hello\n");
    run_git(&seed, &["add", "."]).0;
    run_git(&seed, &["commit", "-m", "extra"]).0;
    let bare_str = bare.to_string_lossy().to_string();
    assert_eq!(run_git(&seed, &["remote", "add", "origin", &bare_str]).0, 0);
    assert_eq!(run_git(&seed, &["push", "-q", "origin", "--all"]).0, 0);

    // Consumer
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", &bare_str]).0, 0);
    let (c0, _o0, _e0) = run_git(&repo, &["show-ref", "--verify", "refs/heads/extra"]);
    assert_ne!(c0, 0);
    run_tool_expect_success(&repo, |o| {
        o.sensitive = true;
    });
    let (c1, _o1, _e1) = run_git(&repo, &["show-ref", "--verify", "refs/heads/extra"]);
    assert_eq!(c1, 0);
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
}

#[test]
fn origin_migration_and_removal_nonsensitive() {
    let repo = init_repo();
    let (_c, headref, _e) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let headref = headref.trim().to_string();
    let branch = headref
        .strip_prefix("refs/heads/")
        .unwrap_or(&headref)
        .to_string();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    let spec = format!("+{}:refs/remotes/origin/{}", headref, branch);
    assert_eq!(run_git(&repo, &["fetch", "origin", &spec]).0, 0);
    run_tool_expect_success(&repo, |_o| {});
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(!remotes.contains("origin"));
}

#[test]
fn sensitive_mode_keeps_origin_remote() {
    let repo = init_repo();
    assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
    run_tool_expect_success(&repo, |o| {
        o.sensitive = true;
        o.no_fetch = true;
        o.no_data = true;
    });
    let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
    assert!(remotes.contains("origin"));
}

#[test]
fn sensitive_mode_validation_rejects_stream_override() {
    use std::path::PathBuf;

    // Test that sensitive mode with stream override fails
    // Use direct validation to avoid "already ran" interactive prompts
    let opts = filter_repo_rs::Options {
        sensitive: true,
        fe_stream_override: Some(PathBuf::from("test_stream")),
        ..Default::default()
    };
    let error = filter_repo_rs::sanity::SensitiveModeValidator::validate_options(&opts)
        .expect_err("sensitive mode with stream override should fail");

    let error_msg = error.to_string();
    assert!(
        error_msg.contains("Sensitive data removal mode is incompatible"),
        "unexpected error: {error_msg}"
    );
}

#[test]
fn sensitive_mode_validation_rejects_custom_paths() {
    use std::path::PathBuf;

    // Test that sensitive mode with custom source fails
    // Use direct validation to avoid "already ran" interactive prompts
    let opts = filter_repo_rs::Options {
        sensitive: true,
        source: PathBuf::from("/custom/source"),
        ..Default::default()
    };
    let error = filter_repo_rs::sanity::SensitiveModeValidator::validate_options(&opts)
        .expect_err("sensitive mode with custom source should fail");

    let error_msg = error.to_string();
    assert!(
        error_msg.contains("Sensitive data removal mode is incompatible"),
        "unexpected error: {error_msg}"
    );

    // Test that sensitive mode with custom target fails
    let opts = filter_repo_rs::Options {
        sensitive: true,
        target: PathBuf::from("/custom/target"),
        ..Default::default()
    };
    let error = filter_repo_rs::sanity::SensitiveModeValidator::validate_options(&opts)
        .expect_err("sensitive mode with custom target should fail");

    let error_msg = error.to_string();
    assert!(
        error_msg.contains("Sensitive data removal mode is incompatible"),
        "unexpected error: {error_msg}"
    );
}
