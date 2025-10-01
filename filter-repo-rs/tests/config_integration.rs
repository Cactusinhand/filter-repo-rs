use filter_repo_rs as fr;
use std::fs;

mod common;
use common::*;

/// Test basic CLI options interaction and analysis configuration
#[test]
fn analyze_config_basic_functionality() {
    let repo = init_repo();

    // Create files to analyze
    write_file(&repo, "small_file.txt", "Small content");
    write_file(&repo, "large_file.txt", &"x".repeat(2 * 1024 * 1024)); // 2MB

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test files for analysis"]);

    // Test basic analysis mode
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.mode = fr::opts::Mode::Analyze;
    opts.analyze.json = false;
    opts.analyze.top = 5;

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn analyze_config_with_custom_thresholds() {
    let repo = init_repo();

    // Create files with various sizes to test thresholds
    write_file(&repo, "file1.txt", "Small content");
    write_file(&repo, "file2.txt", &"x".repeat(1500)); // 1.5KB
    write_file(&repo, "file3.txt", &"x".repeat(5000)); // 5KB

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Files for threshold testing"]);

    // Test analysis with custom thresholds
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.mode = fr::opts::Mode::Analyze;
    opts.analyze.json = false;
    opts.analyze.thresholds.warn_blob_bytes = 2000; // 2KB warning threshold
    opts.analyze.thresholds.warn_total_bytes = 10000; // 10KB total warning
    opts.analyze.thresholds.warn_object_count = 10; // Warn if more than 10 objects

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn analyze_config_json_output() {
    let repo = init_repo();

    // Create test content
    for i in 0..5 {
        write_file(&repo, &format!("file{}.txt", i), &format!("Content {}", i));
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test commit"]);

    // Test JSON analysis output
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.mode = fr::opts::Mode::Analyze;
    opts.analyze.json = true;
    opts.analyze.top = 10;

    let result = fr::run(&opts);
    assert!(result.is_ok());
}

#[test]
fn cli_options_priority_testing() {
    let repo = init_repo();

    // Create test files with different sizes
    write_file(&repo, "small.txt", "Small content");
    write_file(&repo, "medium.txt", &"x".repeat(2000)); // 2KB
    write_file(&repo, "large.txt", &"x".repeat(6000)); // 6KB

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test CLI options"]);

    // Test that larger max_blob_size allows more files through
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.max_blob_size = Some(5000); // 5KB limit

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify files larger than limit were filtered
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

    // Should have small.txt and medium.txt, but not large.txt
    assert!(files.contains(&"small.txt"));
    assert!(files.contains(&"medium.txt"));
    assert!(!files.contains(&"large.txt"));
}

#[test]
fn multiple_cli_options_interaction() {
    let repo = init_repo();

    // Create a complex scenario
    write_file(&repo, "keep_this.txt", "Important content");
    write_file(&repo, "filter_this.txt", &"x".repeat(3000)); // 3KB
    write_file(&repo, "secret.txt", "password: secret123");

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test multiple options"]);

    // Create replacement rules
    let rules_file = repo.join("rules.txt");
    let rules_content = "password==>REDACTED\nsecret==>HIDDEN\n";
    fs::write(&rules_file, rules_content).unwrap();

    // Test multiple options working together
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.max_blob_size = Some(2000); // Filter out larger files
    opts.replace_text_file = Some(rules_file);
    opts.paths = vec![b"keep_this".to_vec()]; // Only keep specific files

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify the combined effect
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

    // Should only have keep_this.txt (path filter + size filter)
    assert!(files.contains(&"keep_this.txt"));
    assert!(!files.contains(&"filter_this.txt"));
    assert!(!files.contains(&"secret.txt"));
}

#[test]
fn path_filtering_with_various_patterns() {
    let repo = init_repo();

    // Create files with different patterns
    let test_files = vec![
        "important_document.txt",
        "temp_file.txt",
        "backup_old.txt",
        "config.json",
        "script.py",
        "data.csv",
        "README.md",
    ];

    for filename in test_files {
        write_file(&repo, filename, "Test content");
    }

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test path filtering patterns"]);

    // Test multiple path filters (OR logic)
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.paths = vec![
        b"important".to_vec(),
        b"config".to_vec(),
        b"README".to_vec(),
    ]; // Keep files matching any of these patterns

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify filtering worked
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

    // Should have files matching the patterns
    assert!(files.contains(&"important_document.txt"));
    assert!(files.contains(&"config.json"));
    assert!(files.contains(&"README.md"));

    // Should not have files not matching patterns
    assert!(!files.contains(&"temp_file.txt"));
    assert!(!files.contains(&"backup_old.txt"));
    assert!(!files.contains(&"script.py"));
    assert!(!files.contains(&"data.csv"));
}

#[test]
fn path_rename_functionality() {
    let repo = init_repo();

    // Create files in a directory structure
    write_file(&repo, "old_dir/file1.txt", "Content 1");
    write_file(&repo, "old_dir/file2.txt", "Content 2");
    write_file(&repo, "old_dir/subdir/file3.txt", "Content 3");

    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Test path rename"]);

    // Test path renaming
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.path_renames = vec![(b"old_dir/".to_vec(), b"new_dir/".to_vec())];

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify paths were renamed
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

    // Should have files with new directory name
    assert!(files.iter().any(|f| f.contains("new_dir/")));
    assert!(!files.iter().any(|f| f.contains("old_dir/")));
}

#[test]
fn branch_and_tag_rename_functionality() {
    let repo = init_repo();

    // Create some commits
    write_file(&repo, "test.txt", "Initial content");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Initial commit"]);

    // Create a tag
    run_git(&repo, &["tag", "v1.0"]);

    // Create a branch
    run_git(&repo, &["checkout", "-b", "feature-branch"]);
    write_file(&repo, "feature.txt", "Feature content");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Feature commit"]);
    run_git(&repo, &["checkout", "main"]);

    // Test tag and branch renaming
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.tag_rename = Some((b"v".to_vec(), b"release-".to_vec()));
    opts.branch_rename = Some((b"feature-".to_vec(), b"feat-".to_vec()));

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Verify renaming worked (tags and branches should be renamed)
    let (_c, tags, _e) = run_git(&repo, &["tag", "-l"]);
    assert!(tags.contains("release-1.0") || tags.contains("release-1.0^{}"));
    assert!(!tags.contains("v1.0"));

    let (_c, branches, _e) = run_git(&repo, &["branch", "-a"]);
    let branch_list: Vec<String> = branches
        .lines()
        .map(|s| s.trim().trim_start_matches('*').trim().to_string())
        .collect();
    assert!(branch_list
        .iter()
        .any(|b| b == "feat-branch" || b == "remotes/origin/feat-branch"));
}

#[test]
fn dry_run_mode_functionality() {
    let repo = init_repo();

    // Create test content
    write_file(&repo, "test.txt", "Original content");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "Original commit"]);

    // Create replacement rules
    let rules_file = repo.join("rules.txt");
    let rules_content = "Original==>Modified\n";
    fs::write(&rules_file, rules_content).unwrap();

    // Test dry run mode
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;
    opts.dry_run = true;
    opts.replace_text_file = Some(rules_file);

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // In dry run mode, the original content should be unchanged
    let (_c, content, _e) = run_git(&repo, &["show", "HEAD:test.txt"]);
    assert!(content.contains("Original content"));
    assert!(!content.contains("Modified content"));
}
