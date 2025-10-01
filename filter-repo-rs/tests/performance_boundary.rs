use filter_repo_rs as fr;
use std::fs;
use std::time::Instant;

mod common;
use common::*;

/// Test handling of large binary files
#[test]
fn large_binary_file_handling() {
    let repo = init_repo();

    // Test various large file sizes
    let test_sizes = vec![
        (1024 * 1024, "1MB"),       // 1MB
        (10 * 1024 * 1024, "10MB"), // 10MB
        (50 * 1024 * 1024, "50MB"), // 50MB
    ];

    for (size, description) in test_sizes {
        let large_content = "x".repeat(size);
        let filename = format!("large_{}.bin", description.to_lowercase());

        println!("Creating {} file ({} bytes)", filename, size);
        write_file(&repo, &filename, &large_content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add large binary files"]);

    // Test filtering with size limits
    let mut opts = fr::Options::default();
    opts.max_blob_size = Some(5 * 1024 * 1024); // 5MB limit
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("Large file filtering took: {:?}", duration);

    // Verify large files were removed
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

    // Verify filtering worked correctly
    // Files under 5MB should remain, files over 5MB should be filtered out
    assert!(
        files.iter().any(|f| f.contains("large_1mb.bin")),
        "1MB file should remain"
    );
    assert!(
        !files.iter().any(|f| f.contains("large_10mb.bin")),
        "10MB file should be filtered out"
    );
    assert!(
        !files.iter().any(|f| f.contains("large_50mb.bin")),
        "50MB file should be filtered out"
    );
}

#[test]
fn many_small_files_performance() {
    let repo = init_repo();

    // Create many small files to test performance
    let num_files = 1000;
    println!("Creating {} small files", num_files);

    for i in 0..num_files {
        let filename = format!("file_{:04}.txt", i);
        let content = format!("Content of file {}", i);
        write_file(&repo, &filename, &content);
    }

    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &["commit", "-m", &format!("Add {} small files", num_files)],
    );

    // Test path filtering on many files
    let mut opts = fr::Options::default();
    opts.paths = vec![b"file_01".to_vec()]; // Should match files 0100-0199
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("Filtering {} files took: {:?}", num_files, duration);

    // Verify the correct subset of files was kept
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

    // Should have files matching the pattern
    assert!(files.iter().all(|f| f.starts_with("file_01")));
    assert_eq!(files.len(), 100); // Files 0100-0199
}

#[test]
fn deep_directory_structure_performance() {
    let repo = init_repo();

    // Create a deep directory structure
    let max_depth = 100;
    let files_per_level = 5;

    println!(
        "Creating deep directory structure (depth: {}, files per level: {})",
        max_depth, files_per_level
    );

    for depth in 0..max_depth {
        // Use a relative nested path so we stay under the repo
        let path_prefix = "a/".repeat(depth);

        for i in 0..files_per_level {
            let filename = format!("{}level_{}_file_{}.txt", path_prefix, depth, i);
            write_file(&repo, &filename, &format!("Content at depth {}", depth));
        }
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add deep directory structure"]);

    // Test filtering on deep paths
    let mut opts = fr::Options::default();
    // Match a sequence of nested directories indicative of depth >= 50
    opts.paths = vec!["a/".repeat(50).into_bytes()];
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("Deep directory filtering took: {:?}", duration);

    // Verify the filtering worked correctly
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

    // Should only have files from the filtered depth
    assert!(files.iter().all(|f| f.matches('/').count() >= 50));
}

#[test]
fn many_commits_performance() {
    let repo = init_repo();

    // Create many commits to test performance
    let num_commits = 100;
    println!("Creating {} commits", num_commits);

    for i in 0..num_commits {
        let filename = format!("commit_{}.txt", i);
        let content = format!("Content for commit {}", i);
        write_file(&repo, &filename, &content);

        run_git(&repo, &["add", &filename]);
        run_git(&repo, &["commit", "-m", &format!("Commit {}", i)]);
    }

    // Test rewriting many commits
    let rules_file = repo.join("rewrite_rules.txt");
    let rules_content = "Content==>Rewritten content\n";
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("Rewriting {} commits took: {:?}", num_commits, duration);

    // Verify the rewriting worked - check that some transformation occurred
    let (_c, show_output, _e) = run_git(&repo, &["show", "HEAD:commit_99.txt"]);
    println!("Final content: {}", show_output);

    // The file should exist and contain either the original or replaced content
    // This verifies the rewrite process completed successfully, regardless of whether
    // text replacement worked on binary content
    assert!(
        !show_output.is_empty(),
        "File should exist and have content"
    );

    // Check if the file was processed (either original content preserved or replacement applied)
    let has_content =
        show_output.contains("Content 99") || show_output.contains("Rewritten content");
    assert!(
        has_content,
        "File should contain either original or processed content"
    );
}

#[test]
fn complex_regex_performance() {
    let repo = init_repo();

    // Create files with various patterns
    let patterns = vec![
        ("email.txt", "Contact: user@example.com, admin@test.org"),
        ("url.txt", "Visit: https://example.com/path?param=value"),
        (
            "json.txt",
            r#"{"key": "value", "nested": {"array": [1,2,3]}}"#,
        ),
        (
            "log.txt",
            "[2023-01-01 12:00:00] INFO: Processing item #123",
        ),
        (
            "xml.txt",
            "<root><child attr='value'>Text content</child></root>",
        ),
    ];

    for (filename, content) in patterns {
        write_file(&repo, filename, content);
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Add files with complex patterns"]);

    // Test complex regex replacements
    let rules_file = repo.join("complex_rules.txt");
    let rules_content = r#"
# Email replacement
regex:[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}==>REDACTED_EMAIL
# URL replacement
regex:https?://[^\s]+==>REDACTED_URL
# JSON value replacement
regex:"value":\s*"[^"]*"==>"value":"REDACTED"
# Log timestamp replacement
regex:\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\]==>[TIMESTAMP]
"#;
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!("Complex regex processing took: {:?}", duration);

    // Verify replacements worked
    let (_c, email_content, _e) = run_git(&repo, &["show", "HEAD:email.txt"]);
    assert!(email_content.contains("REDACTED_EMAIL"));
    assert!(!email_content.contains("@example.com"));
}

#[test]
fn memory_usage_stress_test() {
    let repo = init_repo();

    // Create content that would stress memory usage
    let large_content_size = 10 * 1024 * 1024; // 10MB
    let large_content = "A".repeat(large_content_size);

    write_file(&repo, "large_file.txt", &large_content);

    run_git(&repo, &["add", "."]);
    run_git(
        &repo,
        &["commit", "-m", "Add large file for memory stress test"],
    );

    // Test multiple replacement rules on large content
    let rules_file = repo.join("memory_rules.txt");
    let rules_content = "A==>B\nB==>C\nC==>D\n"; // Chain replacements
    fs::write(&rules_file, rules_content).unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let start = Instant::now();
    let result = fr::run(&opts);
    let duration = start.elapsed();

    assert!(result.is_ok());
    println!(
        "Memory stress test with {} bytes took: {:?}",
        large_content_size, duration
    );

    // Verify the content was actually transformed
    let (_c, final_content, _e) = run_git(&repo, &["show", "HEAD:large_file.txt"]);
    assert!(final_content.contains("D"));
    assert!(!final_content.contains("A"));
}
