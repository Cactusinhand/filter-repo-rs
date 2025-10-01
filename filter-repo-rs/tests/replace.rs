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

#[test]
fn replace_text_glob_redacts_blob() {
    let repo = init_repo();
    write_file(&repo, "data.txt", "foo123 foo999 bar\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add data glob"]).0,
        0
    );
    let repl = repo.join("repl-glob.txt");
    std::fs::write(&repl, "glob:foo*==>Y\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:data.txt"]);
    // The glob foo* is greedy and matches everything from first 'f' to end
    assert!(content.contains("Y\n"), "got: {}", content);
    assert!(!content.contains("foo123"));
    assert!(!content.contains("foo999"));
    assert!(!content.contains("bar"));
}

#[test]
fn replace_text_glob_question_mark_wildcard() {
    let repo = init_repo();
    write_file(&repo, "test.txt", "cat bat rat\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add test"]).0, 0);
    let repl = repo.join("repl-question.txt");
    std::fs::write(&repl, "glob:?at==>MATCH\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:test.txt"]);
    assert!(content.contains("MATCH MATCH MATCH"), "got: {}", content);
    assert!(!content.contains("cat"));
    assert!(!content.contains("bat"));
    assert!(!content.contains("rat"));
}

#[test]
fn replace_text_glob_regex_special_chars_literal() {
    let repo = init_repo();
    write_file(&repo, "special.txt", "a.b c+d e*f g[h]\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add special"]).0, 0);
    let repl = repo.join("repl-special.txt");
    std::fs::write(&repl, "glob:a.b==>DOT\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:special.txt"]);
    assert!(content.contains("DOT c+d e*f g[h]"), "got: {}", content);
    assert!(!content.contains("a.b"));
    // Verify other special chars are NOT treated as regex in glob patterns
    assert!(content.contains("c+d"));
    assert!(content.contains("e*f"));
}

#[test]
fn replace_text_glob_empty_pattern() {
    let repo = init_repo();
    write_file(&repo, "empty.txt", "test content\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add empty"]).0, 0);
    let repl = repo.join("repl-empty.txt");
    std::fs::write(&repl, "glob:==>REPLACED\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:empty.txt"]);
    // Empty glob pattern should match every character (like regex:.*)
    assert!(content.contains("REPLACED"), "got: {}", content);
    assert!(!content.contains("test"));
    assert!(!content.contains("content"));
}

#[test]
fn replace_text_glob_no_replacement_specified() {
    let repo = init_repo();
    write_file(&repo, "default.txt", "secret data\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add default"]).0, 0);
    let repl = repo.join("repl-default.txt");
    std::fs::write(&repl, "glob:secret\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:default.txt"]);
    assert!(content.contains("***REMOVED*** data\n"), "got: {}", content);
    assert!(!content.contains("secret"));
}

#[test]
fn replace_text_mixed_types_in_same_file() {
    let repo = init_repo();
    write_file(&repo, "mixed.txt", "API_KEY_SECRET foo123\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add mixed"]).0, 0);
    let repl = repo.join("repl-mixed.txt");
    // Use patterns that work together: regex (specific) then glob (greedy)
    std::fs::write(&repl, "regex:API_KEY_[A-Z_]+==>REGEX\nglob:foo*==>GLOB\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:mixed.txt"]);
    // Regex matches API_KEY_SECRET first, then glob can match foo*
    assert!(content.contains("REGEX GLOB\n"), "got: {}", content);
    assert!(!content.contains("API_KEY_SECRET"));
    assert!(!content.contains("foo123"));
}

#[test]
fn replace_text_glob_complex_pattern() {
    let repo = init_repo();
    write_file(
        &repo,
        "complex.txt",
        "config-production.yaml config-dev.yaml backup.txt\n",
    );
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add complex"]).0, 0);
    let repl = repo.join("repl-complex.txt");
    std::fs::write(&repl, "glob:config-*.yaml==>CONFIG\n").unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
    let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:complex.txt"]);
    // The glob is greedy and matches from 'config-production' through 'yaml' of the first match
    assert!(content.contains("CONFIG backup.txt\n"), "got: {}", content);
    assert!(!content.contains("config-production"));
    assert!(!content.contains("config-dev"));
}

#[test]
#[should_panic(expected = "invalid UTF-8 in glob rule")]
fn replace_text_glob_invalid_utf8() {
    let repo = init_repo();
    write_file(&repo, "invalid.txt", "test\n");
    run_git(&repo, &["add", "."]).0;
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add invalid"]).0, 0);
    let repl = repo.join("repl-invalid.txt");
    // Create invalid UTF-8 bytes for the glob pattern
    let mut invalid_bytes = b"glob:".to_vec();
    invalid_bytes.extend_from_slice(&[0xFF, 0xFE]); // Invalid UTF-8 sequence
    std::fs::write(&repl, invalid_bytes).unwrap();
    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
    });
}
