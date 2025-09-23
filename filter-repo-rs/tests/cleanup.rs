mod common;
use common::*;

fn run_cleanup_case(
    repo: &std::path::Path,
    args: &[&str],
) -> (std::process::Output, Vec<Vec<String>>) {
    // Add --force to avoid interference from sanity checks in CLI tests
    let mut full_args = vec!["--force"];
    full_args.extend_from_slice(args);
    let (output, invocations) = run_cli_with_git_spy(repo, &full_args);
    (output, git_commands_for_repo(repo, &invocations))
}

#[test]
fn cleanup_modes_trigger_expected_git_commands() {
    let default_repo = init_repo();
    let (default_output, default_cmds) = run_cleanup_case(&default_repo, &[]);
    assert!(
        default_output.status.success(),
        "baseline run should succeed"
    );
    assert!(
        find_git_command(&default_cmds, "reflog").is_none(),
        "default run should not invoke git reflog expire: {:?}",
        default_cmds
    );
    assert!(
        find_git_command(&default_cmds, "gc").is_none(),
        "default run should not invoke git gc: {:?}",
        default_cmds
    );

    let cleanup_repo = init_repo();
    let (cleanup_output, cleanup_cmds) = run_cleanup_case(&cleanup_repo, &["--cleanup"]);
    assert!(
        cleanup_output.status.success(),
        "--cleanup run should succeed"
    );
    let cleanup_reflog = find_git_command(&cleanup_cmds, "reflog")
        .cloned()
        .expect("standard cleanup should expire reflog");
    assert!(
        cleanup_reflog.contains(&"expire".to_string()),
        "reflog invocation should include expire subcommand: {:?}",
        cleanup_reflog
    );
    assert!(
        cleanup_reflog.contains(&"--expire=now".to_string()),
        "standard cleanup should request immediate expire"
    );
    assert!(
        cleanup_reflog.contains(&"--all".to_string()),
        "standard cleanup should expire all refs"
    );
    assert!(
        !cleanup_reflog.contains(&"--expire-unreachable=now".to_string()),
        "standard cleanup should not force unreachable expiry"
    );
    let cleanup_gc = find_git_command(&cleanup_cmds, "gc")
        .cloned()
        .expect("standard cleanup should invoke git gc");
    assert!(
        cleanup_gc.contains(&"--prune=now".to_string()),
        "standard cleanup should prune immediately"
    );
    assert!(
        cleanup_gc.contains(&"--quiet".to_string()),
        "gc should run quietly"
    );
    assert!(
        !cleanup_gc.contains(&"--aggressive".to_string()),
        "standard cleanup should not request aggressive gc"
    );

    let aggressive_repo = init_repo();
    let (aggressive_output, aggressive_cmds) =
        run_cleanup_case(&aggressive_repo, &["--debug-mode", "--cleanup-aggressive"]);
    assert!(
        aggressive_output.status.success(),
        "--cleanup-aggressive run should succeed"
    );
    let aggressive_reflog = find_git_command(&aggressive_cmds, "reflog")
        .cloned()
        .expect("aggressive cleanup should expire reflog");
    assert!(
        aggressive_reflog.contains(&"--expire-unreachable=now".to_string()),
        "aggressive cleanup should expire unreachable entries"
    );
    let aggressive_gc = find_git_command(&aggressive_cmds, "gc")
        .cloned()
        .expect("aggressive cleanup should invoke git gc");
    assert!(
        aggressive_gc.contains(&"--aggressive".to_string()),
        "aggressive cleanup should request aggressive gc"
    );

    let dry_repo = init_repo();
    let (dry_output, dry_cmds) = run_cleanup_case(&dry_repo, &["--cleanup", "--dry-run"]);
    assert!(
        dry_output.status.success(),
        "dry-run cleanup should succeed"
    );
    assert!(
        find_git_command(&dry_cmds, "reflog").is_none(),
        "dry-run should skip reflog expire even with --cleanup: {:?}",
        dry_cmds
    );
    assert!(
        find_git_command(&dry_cmds, "gc").is_none(),
        "dry-run should skip git gc even with --cleanup: {:?}",
        dry_cmds
    );
}
