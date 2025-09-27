mod common;
use common::*;

fn has_arg(cmd: &[String], needle: &str) -> bool {
    cmd.iter().any(|a| a == needle)
}

#[test]
fn early_untracked_fails_fast_and_skips_heavy_scans() {
    let repo = init_repo();
    // Create an untracked file
    write_file(&repo, "rs.txt", "x");

    // Run CLI with git-spy to capture invocations
    let (out, inv) = run_cli_with_git_spy(&repo, &["--max-blob-size", "1024", "--write-report"]);
    assert!(
        !out.status.success(),
        "expected preflight to fail on untracked files"
    );

    // Ensure we did not run heavy ref/reflog scans before failing
    let cmds = git_commands_for_repo(&repo, &inv);
    let ran_show_ref = cmds.iter().any(|c| has_arg(c, "show-ref") || has_arg(c, "for-each-ref"));
    let ran_reflog = cmds.iter().any(|c| c.iter().any(|a| a == "reflog"));
    assert!(
        !ran_show_ref && !ran_reflog,
        "heavy scans should be skipped on early failure: {:?}",
        cmds
    );
}

#[test]
fn early_dirty_fails_fast() {
    let repo = init_repo();
    // Make unstaged modification
    write_file(&repo, "README.md", "dirty");

    let (out, inv) = run_cli_with_git_spy(&repo, &[]);
    assert!(
        !out.status.success(),
        "expected preflight to fail on unstaged changes"
    );
    let cmds = git_commands_for_repo(&repo, &inv);
    let ran_show_ref = cmds.iter().any(|c| c.iter().any(|a| a == "show-ref"));
    assert!(!ran_show_ref, "should fail before ref scans: {:?}", cmds);
}

