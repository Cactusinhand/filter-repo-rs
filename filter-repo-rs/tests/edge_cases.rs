use filter_repo_rs as fr;
use std::collections::BTreeSet;
use std::fs;

mod common;
use common::*;

/// Test edge cases for blob filtering
#[test]
fn blob_filtering_edge_cases() {
    let repo = init_repo();

    // Test exact size boundary conditions
    let boundary_cases = vec![
        (999, "just_under_1KB"),
        (1000, "exactly_1KB"),
        (1001, "just_over_1KB"),
        (1024 * 1024 - 1, "just_under_1MB"),
        (1024 * 1024, "exactly_1MB"),
        (1024 * 1024 + 1, "just_over_1MB"),
    ];

    for (size, name) in boundary_cases {
        let content = "x".repeat(size);
        let filename = format!("{}.txt", name);
        write_file(&repo, &filename, &content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test boundary sizes"]);

    // Test with exactly 1KB limit
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(1000);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify boundary behavior
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();

    // Should include exactly_1KB but not just_over_1KB
    assert!(files.iter().any(|f| f.contains("exactly_1KB")));
    assert!(!files.iter().any(|f| f.contains("just_over_1KB")));
}

#[test]
fn empty_and_whitespace_files() {
    let repo = init_repo();

    // Create files with various edge cases
    let edge_case_files = vec![
        ("empty.txt", ""),
        ("spaces.txt", "   "),
        ("tabs.txt", "\t\t\t"),
        ("mixed_whitespace.txt", " \t \n \r\n "),
        ("only_newline.txt", "\n"),
        ("only_carriage_return.txt", "\r"),
        ("crlf.txt", "\r\n"),
        ("unicode_whitespace.txt", "\u{2000}\u{2001}\u{2002}"), // Unicode spaces
    ];

    for (filename, content) in &edge_case_files {
        write_file(&repo, filename, content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test whitespace edge cases"]);

    // Test content replacement on empty/whitespace files
    let rules_file = repo.join("whitespace_rules.txt");
    let rules_content = " ==>REPLACED\n\t==>TAB\n"; // Try to replace empty string and tabs
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Files should still exist (no crashes on empty content)
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: BTreeSet<&str> = tree.lines().filter(|name| *name != "README.md").collect();
    let expected: BTreeSet<&str> = edge_case_files.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        files, expected,
        "All edge case files should exist after processing"
    );
}

#[test]
fn special_characters_in_replacements() {
    let repo = init_repo();

    // Create content with various special characters
    let special_content = r#"
Test content with special characters:
Quotes: "single" and 'double'
Brackets: [square], {curly}, (parentheses)
Math: + - * / ^ %
Punctuation: ! @ # $ % ^ & * ( ) _ - + = { } [ ] | \ : ; " ' < > , . ? /
HTML: &lt; &gt; &amp; &quot;
Unicode: café, naïve, résumé, 🚀, 💡
Escaped: \n \t \r \\ \" \'
"#;
    write_file(&repo, "special.txt", special_content);

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test special characters"]);

    // Create replacement rules with special characters
    let rules_file = repo.join("special_rules.txt");
    let rules_content = r#"
# Test special characters in replacement patterns
"single"==>'double'
regex:\[square\]==><curly>
+==>PLUS
%==>PERCENT
café==>CAFE
🚀==>ROCKET
\n==>NEWLINE
\t==>TAB
&lt;==>AMP_LT
"#;
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify replacements worked
    let (_c, content, _e) = run_git(&repo, &["show", "HEAD:special.txt"]);
    assert!(content.contains("'double'"));
    assert!(content.contains("<curly>"));
    assert!(content.contains("PLUS"));
    assert!(content.contains("PERCENT"));
    assert!(content.contains("CAFE"));
    assert!(content.contains("ROCKET"));
    assert!(content.contains("NEWLINE"));
    assert!(content.contains("TAB"));
    assert!(content.contains("AMP_LT"));
}

#[test]
fn path_filtering_with_special_patterns() {
    // Create files with patterns that work across platforms
    let pattern_files = vec![
        "file_with_spaces.txt",
        "file-with-dashes.txt",
        "file_with_underscores.txt",
        "file.with.dots.txt",
        "file_brackets.txt",
        "file_braces.txt",
        "file_quotes.txt",
        "file_hash.txt",
        "file_at.txt",
        "file_plus.txt",
        "file_percent.txt",
    ];

    // Test various path filtering patterns
    let test_patterns = vec![
        ("file_with", "underscore pattern"),
        ("file-with", "dash pattern"),
        ("file.with", "dot pattern"),
        ("file_brackets", "brackets pattern"),
    ];

    for (pattern, description) in test_patterns {
        let repo = init_repo();

        for filename in &pattern_files {
            write_file(&repo, filename, "test content");
        }

        run_git(&repo, &["add", "."]);
        run_git(
            &repo,
            &[
                "commit",
                "-m",
                &format!("Populate files for {}", description),
            ],
        );

        let mut opts = fr::Options::default();
        opts.paths = vec![pattern.as_bytes().to_vec()];
        opts.source = repo.clone();
        opts.target = repo.clone();
        opts.force = true;

        // Create branch for each test
        let branch_name = format!("test_{}", description.replace(' ', "_"));
        run_git(&repo, &["checkout", "-b", &branch_name]);

        let result = fr::run(&opts);
        assert!(result.is_ok(), "Failed for pattern: {}", pattern);

        let (_code, tree, _stderr) = run_git(
            &repo,
            &[
                "-c",
                "core.quotepath=false",
                "ls-tree",
                "-r",
                "--name-only",
                "HEAD",
            ],
        );
        let actual: BTreeSet<String> = tree
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();

        let expected: BTreeSet<String> = match pattern {
            "file_with" => BTreeSet::from([
                "file_with_spaces.txt".to_string(),
                "file_with_underscores.txt".to_string(),
            ]),
            "file-with" => BTreeSet::from(["file-with-dashes.txt".to_string()]),
            "file.with" => BTreeSet::from(["file.with.dots.txt".to_string()]),
            "file_brackets" => BTreeSet::from(["file_brackets.txt".to_string()]),
            _ => panic!("unexpected pattern: {}", pattern),
        };

        assert_eq!(
            actual, expected,
            "Pattern `{}` should keep expected files ({})",
            pattern, description
        );

        let unexpected: Vec<String> = pattern_files
            .iter()
            .map(|name| name.to_string())
            .filter(|name| !expected.contains(name))
            .collect();

        for removed in unexpected {
            assert!(
                !actual.contains(&removed),
                "Pattern `{pattern}` should remove `{removed}`"
            );
        }
    }
}

#[test]
fn binary_file_handling_edge_cases() {
    let repo = init_repo();

    // Create various binary file types
    let binary_cases = vec![
        (vec![0u8; 100], "all_zeros.bin"),
        (vec![255u8; 100], "all_ones.bin"),
        (vec![0u8, 1u8, 2u8, 3u8].repeat(25), "sequential.bin"),
        (vec![0xFF, 0xFE, 0xFD].repeat(34), "decreasing.bin"),
        (
            (0..100).map(|i| (i % 256) as u8).collect::<Vec<u8>>(),
            "cyclic.bin",
        ),
    ];

    for (content, filename) in &binary_cases {
        let path = repo.join(filename);
        fs::write(path, content).unwrap();
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test binary files"]);

    // Test content replacement on binary files (should not corrupt)
    let rules_file = repo.join("binary_rules.txt");
    let rules_content = "0==>1\n255==>254\n"; // Try to replace binary patterns
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Files should still exist and be accessible
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();
    // Files should still exist and be accessible
    assert!(!files.is_empty(), "Binary files should still be accessible");
}

#[test]
fn commit_message_edge_cases() {
    let repo = init_repo();

    // Create commits with edge case messages
    let long_message = "A".repeat(1000);
    let edge_case_messages = vec![
        "",            // Empty message
        " ",           // Single space
        "\n",          // Single newline
        "   \n   ",    // Whitespace only
        &long_message, // Very long message
        "Multi\nLine\nMessage\nWith\nNewlines",
        "Message with \"quotes\" and 'apostrophes'",
        "Message with [brackets] {braces} (parentheses)",
        "Message with &lt;HTML&gt; entities",
        "Message with café naïve résumé",
        "Message with 🚀 emoji and 💡 symbols",
    ];

    for (i, message) in edge_case_messages.iter().enumerate() {
        write_file(&repo, &format!("file{}.txt", i), &format!("Content {}", i));
        run_git(&repo, &["add", "."]);

        let commit_args = if message.is_empty() {
            vec!["commit", "--allow-empty-message", "-m", ""]
        } else {
            vec!["commit", "-m", message]
        };

        run_git(&repo, &commit_args);
    }

    // Test message replacement on edge cases
    let rules_file = repo.join("message_rules.txt");
    let rules_content = r#"
Multi==>MULTI_REPLACED
quotes==>QUOTES
café==>CAFE
🚀==>ROCKET
<==>LT
>==>GT
"#;
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify messages were replaced across full bodies
    let (_c, log_output, _e) = run_git(&repo, &["log", "--format=%B", "-n", "50"]);
    // Expect specific replacements to occur and originals to be gone
    assert!(
        log_output.contains("MULTI_REPLACED"),
        "expected 'Multi' to be replaced"
    );
    assert!(
        !log_output.contains("Multi"),
        "unexpected original token 'Multi' present"
    );
    assert!(
        log_output.contains("QUOTES"),
        "expected 'quotes' to be replaced"
    );
    assert!(
        !log_output.contains("quotes"),
        "unexpected original token 'quotes' present"
    );
    assert!(
        log_output.contains("CAFE"),
        "expected 'café' to be replaced"
    );
    assert!(
        !log_output.contains("café"),
        "unexpected original token 'café' present"
    );
    assert!(
        log_output.contains("ROCKET"),
        "expected '🚀' to be replaced"
    );
    assert!(
        !log_output.contains("🚀"),
        "unexpected original token '🚀' present"
    );
}

#[test]
fn concurrent_git_operations_simulation() {
    let repo = init_repo();

    // Create a more complex history to simulate concurrent scenarios
    for i in 0..10 {
        // Create multiple branches
        let branch_name = format!("feature_{}", i);
        run_git(&repo, &["checkout", "-b", &branch_name]);

        for j in 0..5 {
            write_file(
                &repo,
                &format!("file_{}_{}.txt", i, j),
                &format!("Content {}-{}", i, j),
            );
            run_git(&repo, &["add", "."]);
            run_git(&repo, &["commit", "-m", &format!("Commit {}-{}", i, j)]);
        }

        run_git(&repo, &["checkout", "main"]);
    }

    // Merge all branches to create complex history
    // Handle merge failures gracefully by checking results
    let mut successful_merges = 0;
    for i in 0..10 {
        let branch_name = format!("feature_{}", i);
        let (exit_code, _output, _error) = run_git(
            &repo,
            &[
                "merge",
                &branch_name,
                "--no-ff",
                "-m",
                &format!("Merge {}", branch_name),
            ],
        );

        if exit_code == 0 {
            successful_merges += 1;
        } else {
            // If merge fails, continue with the test - this still exercises complex history processing
            println!("Merge of {} failed, continuing test", branch_name);
        }
    }

    // Ensure we have at least some merges for a meaningful test
    if successful_merges < 3 {
        println!(
            "Only {}/10 merges succeeded, creating additional commits for test complexity",
            successful_merges
        );

        // Add some commits directly to main if merges failed
        for i in 0..5 {
            write_file(
                &repo,
                &format!("extra_file_{}.txt", i),
                &format!("Extra content {}", i),
            );
            run_git(&repo, &["add", "."]);
            run_git(&repo, &["commit", "-m", &format!("Extra commit {}", i)]);
        }
    }

    // Test filtering on complex history
    let mut opts = fr::Options::default();
    opts.paths = vec![b"file_".to_vec()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify the result is consistent - check that filtering was applied
    let (_c, tree, _e) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files: Vec<&str> = tree.split_whitespace().collect();

    // The test should have filtered files, so remaining files should match our pattern
    // If the pattern was "file_", all remaining files should start with "file_"
    let non_matching_files: Vec<_> = files
        .iter()
        .filter(|f| !f.starts_with("file_") && !f.starts_with("extra_file_"))
        .collect();

    assert!(
        non_matching_files.is_empty(),
        "All remaining files should match the filter pattern. Found non-matching: {:?}",
        non_matching_files
    );

    // Verify we have some files (not everything was filtered out)
    assert!(!files.is_empty(), "Should have some files after filtering");

    // Check that our path filtering actually worked by verifying the pattern match
    let matching_files: Vec<_> = files.iter().filter(|f| f.starts_with("file_")).collect();

    assert!(
        !matching_files.is_empty(),
        "Should have files matching 'file_' pattern"
    );
}

#[test]
fn malformed_replacement_rules() {
    #[derive(Debug)]
    struct RuleCase<'a> {
        filename: &'a str,
        rules_content: &'a str,
        initial_content: &'a str,
        expected_content: &'a str,
        description: &'a str,
    }

    let malformed_rules = vec![
        RuleCase {
            filename: "missing_separator.txt",
            rules_content: "pattern_without_separator",
            initial_content: "pattern_without_separator should be replaced",
            expected_content: "***REMOVED*** should be replaced",
            description: "lines without '==>' fall back to ***REMOVED***",
        },
        RuleCase {
            filename: "empty_pattern.txt",
            rules_content: "==>replacement",
            initial_content: "content should remain unchanged",
            expected_content: "content should remain unchanged",
            description: "empty patterns are ignored",
        },
        RuleCase {
            filename: "empty_replacement.txt",
            rules_content: "pattern==>",
            initial_content: "pattern should disappear",
            expected_content: " should disappear",
            description: "empty replacements drop the matched pattern",
        },
        RuleCase {
            filename: "multiple_separators.txt",
            rules_content: "pattern==>too==>many==>separators",
            initial_content: "pattern becomes verbose",
            expected_content: "too==>many==>separators becomes verbose",
            description: "only the first separator splits pattern and replacement",
        },
        RuleCase {
            filename: "unicode_bom.txt",
            rules_content: "\u{FEFF}pattern==>replacement",
            initial_content: "pattern without bom remains",
            expected_content: "pattern without bom remains",
            description: "BOM-prefixed rules do not match plain text",
        },
        RuleCase {
            filename: "trailing_newlines.txt",
            rules_content: "pattern==>replacement\n\n\n",
            initial_content: "pattern should be rewritten",
            expected_content: "replacement should be rewritten",
            description: "trailing blank lines are ignored",
        },
    ];

    for case in malformed_rules {
        let repo = init_repo();
        write_file(&repo, "test.txt", case.initial_content);
        // Stage and commit the test file, handling any failures gracefully
        let (add_code, add_output, add_error) = run_git(&repo, &["add", "test.txt"]);
        if add_code != 0 {
            panic!(
                "Failed to stage test content for {}: code={}, output={}, error={}",
                case.filename, add_code, add_output, add_error
            );
        }

        let (commit_code, commit_output, commit_error) = run_git(
            &repo,
            &["commit", "-m", &format!("setup {}", case.filename)],
        );
        if commit_code != 0 {
            panic!(
                "Failed to commit setup for {}: code={}, output={}, error={}",
                case.filename, commit_code, commit_output, commit_error
            );
        }

        let rules_file = repo.join(case.filename);
        fs::write(&rules_file, case.rules_content).unwrap();

        let mut opts = fr::Options::default();
        opts.replace_text_file = Some(rules_file);
        opts.source = repo.clone();
        opts.target = repo.clone();
        opts.force = true;

        let result = fr::run(&opts);
        assert!(
            result.is_ok(),
            "expected malformed rule {} to succeed: {}",
            case.filename,
            case.description
        );

        let (_code, show_output, _err) = run_git(&repo, &["show", "HEAD:test.txt"]);
        assert_eq!(
            show_output, case.expected_content,
            "unexpected rewritten content for {} ({})",
            case.filename, case.description
        );
    }
}
