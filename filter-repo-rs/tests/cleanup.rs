mod common;
use common::*;

fn contains_seq(cmd: &[String], seq: &[&str]) -> bool {
    if seq.is_empty() {
        return true;
    }
    let mut i = 0usize;
    for part in cmd {
        if part == seq[i] {
            i += 1;
            if i == seq.len() {
                return true;
            }
        }
    }
    false
}

fn any_cmd_contains_seq(cmds: &[Vec<String>], seq: &[&str]) -> bool {
    cmds.iter().any(|c| contains_seq(c, seq))
}

#[test]
fn default_cleanup_runs_standard() {
    let repo = init_repo();
    let (out, inv) = run_cli_with_git_spy(&repo, &[]);
    assert!(out.status.success(), "run should succeed");
    let cmds = git_commands_for_repo(&repo, &inv);

    // Expect reflog expire --expire=now --all
    assert!(
        any_cmd_contains_seq(&cmds, &["reflog", "expire", "--expire=now", "--all"]),
        "expected reflog expire in cleanup; cmds: {:?}",
        cmds
    );
    // Expect git gc --prune=now (quiet flag may be present)
    assert!(
        any_cmd_contains_seq(&cmds, &["gc", "--prune=now"])
            || any_cmd_contains_seq(&cmds, &["gc", "--prune=now", "--quiet"])
            || any_cmd_contains_seq(&cmds, &["gc", "--quiet", "--prune=now"]),
        "expected git gc --prune=now in cleanup; cmds: {:?}",
        cmds
    );
    // Should not be aggressive by default
    assert!(
        !any_cmd_contains_seq(&cmds, &["gc", "--aggressive"]),
        "did not expect aggressive gc by default; cmds: {:?}",
        cmds
    );
}

#[test]
fn cleanup_aggressive_runs_when_requested() {
    let repo = init_repo();
    // --cleanup-aggressive is gated behind debug-mode
    let (out, inv) = run_cli_with_git_spy(&repo, &["--debug-mode", "--cleanup-aggressive"]);
    assert!(out.status.success(), "run should succeed");
    let cmds = git_commands_for_repo(&repo, &inv);

    // Aggressive should include extra expire-unreachable and gc --aggressive
    assert!(
        any_cmd_contains_seq(
            &cmds,
            &[
                "reflog",
                "expire",
                "--expire=now",
                "--expire-unreachable=now",
                "--all"
            ]
        ),
        "expected aggressive reflog expire; cmds: {:?}",
        cmds
    );
    assert!(
        any_cmd_contains_seq(&cmds, &["gc", "--prune=now", "--aggressive"])
            || any_cmd_contains_seq(&cmds, &["gc", "--aggressive", "--prune=now"]),
        "expected gc --aggressive; cmds: {:?}",
        cmds
    );
}

#[test]
fn cleanup_disabled_on_dry_run() {
    let repo = init_repo();
    let (out, inv) = run_cli_with_git_spy(&repo, &["--dry-run"]);
    // dry-run returns success; we only check that no cleanup ran
    assert!(out.status.success(), "dry-run should succeed");
    let cmds = git_commands_for_repo(&repo, &inv);
    assert!(
        !any_cmd_contains_seq(&cmds, &["reflog", "expire"])
            && !any_cmd_contains_seq(&cmds, &["gc"]),
        "cleanup should not run on dry-run; cmds: {:?}",
        cmds
    );
}

#[test]
fn cleanup_disabled_on_partial() {
    let repo = init_repo();
    let (out, inv) = run_cli_with_git_spy(&repo, &["--partial"]);
    assert!(out.status.success(), "partial run should succeed");
    let cmds = git_commands_for_repo(&repo, &inv);
    assert!(
        !any_cmd_contains_seq(&cmds, &["reflog", "expire"])
            && !any_cmd_contains_seq(&cmds, &["gc"]),
        "cleanup should not run on partial; cmds: {:?}",
        cmds
    );
}
