mod common;
use common::*;

// Ensure --replace-message supports regex rules via lines beginning with 'regex:'
// and can remove generic Co-authored-by lines across commit messages.
#[test]
fn remove_co_authored_by_lines_with_regex() {
    let repo = init_repo();

    // Create a commit with a Co-authored-by trailer line
    write_file(&repo, "README.md", "test");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    let msg1 = "Update README.zh-CN.md";
    let msg2 = "    Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>";
    assert_eq!(
        run_git(&repo, &["commit", "-m", msg1, "-m", msg2]).0,
        0,
        "failed to create commit with Co-authored-by trailer"
    );

    // Regex rule: remove any Co-authored-by line (use (?m) for multi-line)
    let rules = repo.join("message_regex_rules.txt");
    std::fs::write(&rules, "regex:(?m)^\\s*Co-authored-by:.*$==>\n").unwrap();

    run_tool_expect_success(&repo, |o| {
        o.replace_message_file = Some(rules.clone());
    });

    // Verify the last commit message no longer contains the trailer
    let (_c, body, _e) = run_git(&repo, &["log", "-1", "--format=%B"]);
    assert!(body.contains("Update README.zh-CN.md"));
    assert!(
        !body.contains("Co-authored-by:"),
        "Co-authored-by line should be removed by regex rule"
    );
}
