use filter_repo_rs as fr;
use std::fs;

mod common;
use common::*;

/// Tests for Windows-specific path handling and sanitization
#[cfg(windows)]
#[test]
fn windows_path_sanitization_reserved_characters() {
    let repo = init_repo();

    // Create files with names that simulate Windows path sanitization scenarios
    // Note: We create safe versions of the names since we can't create actual invalid filenames
    let test_names = vec![
        "con_sanitized.txt",
        "prn_sanitized.doc",
        "aux_sanitized.dat",
        "nul_sanitized.log",
        "com1_sanitized.txt",
        "com2_sanitized.bat",
        "lpt1_sanitized.doc",
        "lpt9_sanitized.log",
        "file_name_sanitized.txt",
        "file_name_sanitized.doc",
        "file_name_sanitized.dat",
        "file_name_sanitized.log",
        "file_name_sanitized.tmp",
        "file_name_sanitized.bak",
    ];

    for name in test_names {
        write_file(&repo, name, "test content");
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add files with sanitized names"]);

    // Test path filtering works with sanitized paths
    let mut opts = fr::Options::default();
    opts.paths = vec![b"sanitized".to_vec()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn path_handling_trailing_dots_and_spaces() {
    let repo = init_repo();

    // Test handling of files with trailing dots and spaces
    let test_cases = vec![
        ("file.txt.", "file.txt"),
        ("file.txt  ", "file.txt"),
        ("file.txt . ", "file.txt"),
        ("normal_file.txt", "normal_file.txt"),
    ];

    for (original, expected_sanitized) in test_cases {
        let path = if cfg!(windows) {
            expected_sanitized
        } else {
            original
        };

        write_file(&repo, path, &format!("Content for {}", original));
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test trailing dots and spaces"]);

    // Verify path filtering works
    let mut opts = fr::Options::default();
    opts.paths = vec![b"file.txt".to_vec()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn unicode_path_filtering() {
    let repo = init_repo();

    let unicode_cases = vec![
        ("文件.txt", "Chinese filename"),
        ("файл.txt", "Cyrillic filename"),
        ("🚀rocket.txt", "Emoji filename"),
        ("café.txt", "Accented characters"),
        ("ファイル.txt", "Japanese filename"),
        ("ملف.txt", "Arabic filename"),
        ("test'.txt", "UTF-8 encoded apostrophe"),
    ];

    for (filename, content) in unicode_cases {
        write_file(&repo, filename, content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add Unicode files"]);

    let mut opts = fr::Options::default();
    opts.paths = vec!["文件".as_bytes().to_vec()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    run_git(&repo, &["config", "core.quotePath", "false"]);
    let (_status, tree, _err) = run_git(&repo, &["ls-tree", "--name-only", "HEAD"]);
    let files: Vec<_> = tree.lines().collect();
    assert_eq!(files, vec!["文件.txt"]);
}

#[test]
fn unicode_content_replacement() {
    let repo = init_repo();

    let unicode_files = vec![
        ("café.txt", "Enjoy a beverage at the café."),
        ("rocket.txt", "Launch 🚀 soon."),
        ("文件.txt", "The 文件 contains mixed content."),
        ("notes.txt", "A regular ASCII file."),
    ];

    for (filename, content) in unicode_files {
        write_file(&repo, filename, content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add Unicode content"]);

    let rules_file = repo.join("unicode_rules.txt");
    let rules_content = "café==>cafe\n🚀==>rocket\n文件==>file\n";
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    let (_c, cafe_content, _e) = run_git(&repo, &["show", "HEAD:café.txt"]);
    assert!(cafe_content.contains("cafe"));
    assert!(!cafe_content.contains("café"));

    let (_c, rocket_content, _e) = run_git(&repo, &["show", "HEAD:rocket.txt"]);
    assert!(rocket_content.contains("rocket"));
    assert!(!rocket_content.contains("🚀"));

    let (_c, file_content, _e) = run_git(&repo, &["show", "HEAD:文件.txt"]);
    assert!(file_content.contains("file"));
    assert!(!file_content.contains("文件"));

    let (_c, notes_content, _e) = run_git(&repo, &["show", "HEAD:notes.txt"]);
    assert!(notes_content.contains("regular ASCII"));
    assert!(!notes_content.contains("rocket"));
}

#[test]
fn extreme_path_length_handling() {
    let repo = init_repo();

    // Test very long path names (Windows limit: 260 chars, extended path: ~32767)
    let (segment_len, base_len, long_filename_len) = if cfg!(windows) {
        // Keep generated paths comfortably below MAX_PATH on Windows so git
        // commands succeed without requiring opt-in long-path support.
        (20, 40, 60)
    } else {
        (50, 100, 200)
    };
    let base_name = "a".repeat(base_len);
    let deep_path = format!(
        "{}/{}/{}/{}",
        "a".repeat(segment_len),
        "b".repeat(segment_len),
        "c".repeat(segment_len),
        base_name
    );

    let long_filename = "a".repeat(long_filename_len);
    #[cfg(windows)]
    {
        let deep_len = repo.join(&deep_path).to_string_lossy().chars().count();
        let long_len = repo.join(&long_filename).to_string_lossy().chars().count();
        assert!(
            deep_len < 240,
            "test-generated deep path should stay within Windows MAX_PATH"
        );
        assert!(
            long_len < 240,
            "test-generated long filename should stay within Windows MAX_PATH"
        );
    }

    write_file(&repo, &deep_path, "Deep path content");
    write_file(&repo, &long_filename, "Long filename content");

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test extreme path lengths"]);

    // Test that path filtering works with long paths
    let mut opts = fr::Options::default();
    opts.paths = vec![b"a".to_vec()]; // Should match long paths starting with 'a'
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn mixed_line_endings_handling() {
    let repo = init_repo();

    // Test files with different line endings
    let content_crlf = "line1\r\nline2\r\nline3\r\n";
    let content_lf = "line1\nline2\nline3\n";
    let content_mixed = "line1\r\nline2\nline3\r\nline4\n";
    let content_cr = "line1\rline2\rline3\r";

    write_file(&repo, "crlf.txt", content_crlf);
    write_file(&repo, "lf.txt", content_lf);
    write_file(&repo, "mixed.txt", content_mixed);
    write_file(&repo, "cr.txt", content_cr);

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test mixed line endings"]);

    // Test content replacement across different line endings
    let rules_file = repo.join("line_ending_rules.txt");
    let rules_content = "line1==>replaced_line1\nline2==>replaced_line2\n";
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify the content was replaced regardless of line ending type
    let (_c, content, _e) = run_git(&repo, &["show", "HEAD:crlf.txt"]);
    assert!(content.contains("replaced_line1") && content.contains("replaced_line2"));
}

#[test]
fn case_insensitive_path_handling() {
    let repo = init_repo();

    // Create files with different cases
    write_file(&repo, "Test.txt", "Uppercase content");
    write_file(&repo, "test.txt", "Lowercase content");
    write_file(&repo, "TeSt.txt", "Mixed case content");

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test case handling"]);

    // Test path filtering with different cases
    let test_cases = vec!["test", "TEST", "TeSt", "test.TXT"];

    for case in test_cases {
        let mut opts = fr::Options::default();
        opts.paths = vec![case.as_bytes().to_vec()];
        opts.source = repo.clone();
        opts.target = repo.clone();
        opts.force = true;

        // Create a new branch for each test to avoid conflicts
        let branch_name = format!("test_{}", case.to_lowercase());
        run_git(&repo, &["checkout", "-b", &branch_name]);

        let result = fr::run(&opts);
        assert!(result.is_ok(), "Failed for case pattern: {}", case);
    }
}
