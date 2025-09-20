mod common;
use common::*;

#[test]
fn replace_text_redacts_blob_contents() {
    let repo = init_repo();
    write_file(&repo, "secret.txt", "token=SECRET-ABC-123\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add secret"]).0, 0);
    let repl = repo.join("repl-blobs.txt");
    std::fs::write(&repl, "SECRET-ABC-123==>REDACTED\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:secret.txt"]);
    assert!(content.contains("REDACTED"));
    assert!(!content.contains("SECRET-ABC-123"));
}

#[test]
fn replace_text_regex_redacts_blob() {
    let repo = init_repo();
    write_file(&repo, "data.txt", "foo123 foo999\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add data"]).0, 0);
    let repl = repo.join("repl-regex.txt");
    std::fs::write(&repl, "regex:foo[0-9]+==>X\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:data.txt"]);
    assert!(content.contains("X X"));
    assert!(!content.contains("foo123"));
}
