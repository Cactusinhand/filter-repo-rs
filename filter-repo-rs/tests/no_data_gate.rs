mod common;
use common::*;

fn has_arg(cmd: &[String], needle: &str) -> bool {
    cmd.iter().any(|a| a == needle)
}

#[test]
fn auto_no_data_enabled_when_safe_and_useful() {
    let repo = init_repo();
    // Add a moderately large file so that --max-blob-size matters
    write_file(&repo, "large.bin", &"x".repeat(4096));
    run_git(&repo, &["add", "."]);
    assert_eq!(run_git(&repo, &["commit", "-m", "add large"]).0, 0);

    let (out, inv) = run_cli_with_git_spy(&repo, &["--max-blob-size", "1024", "--force"]);
    assert!(out.status.success(), "run should succeed");
    let cmds = git_commands_for_repo(&repo, &inv);
    // Find fast-export invocation
    let fe = cmds
        .iter()
        .find(|c| c.iter().any(|a| a == "fast-export"))
        .expect("fast-export was invoked");
    assert!(
        has_arg(fe, "--no-data"),
        "expected auto --no-data in fast-export args: {:?}",
        fe
    );
}
