use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn error_handling_invalid_source_repository() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let invalid_repo = temp_dir.path().join("nonexistent");

    let opts = fr::Options {
        source: invalid_repo.clone(),
        target: temp_dir.path().to_path_buf(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        ..Default::default()
    };

    let result = fr::run(&opts);
    assert!(result.is_err());
    let error = result.err().unwrap();
    let error_msg = format!("{:?}", error);
    assert!(
        error_msg.contains("not a git repo") || error_msg.contains("failed"),
        "unexpected error: {}",
        error_msg
    );
}

#[test]
fn error_handling_invalid_target_repository() {
    let repo = init_repo();
    let temp_dir = tempfile::TempDir::new().unwrap();
    let invalid_target = temp_dir
        .path()
        .join("nonexistent")
        .join("nested")
        .join("path");

    let opts = fr::Options {
        source: repo.clone(),
        target: invalid_target,
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        ..Default::default()
    };

    let result = fr::run(&opts);
    assert!(result.is_err());
}

#[test]
fn error_handling_nonexistent_replace_message_file() {
    let repo = init_repo();
    let nonexistent_file = repo.join("nonexistent_replacements.txt");

    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        replace_message_file: Some(nonexistent_file),
        ..Default::default()
    };

    let result = fr::run(&opts);
    assert!(result.is_err());
}

#[test]
fn error_handling_invalid_sha_format_in_strip_blobs() {
    let repo = init_repo();
    let invalid_sha_file = repo.join("invalid_shas.txt");
    std::fs::write(&invalid_sha_file, "invalid123\nnotahash\nshort\n").unwrap();

    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "test commit"]);

    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        strip_blobs_with_ids: Some(invalid_sha_file),
        ..Default::default()
    };

    let _result = fr::run(&opts);
}

#[test]
fn path_rename_with_identical_paths() {
    let repo = init_repo();
    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        path_renames: vec![(b"invalidformat".to_vec(), b"invalidformat".to_vec())],
        ..Default::default()
    };
    let _result = fr::run(&opts);
}

#[test]
fn error_handling_invalid_max_blob_size_values() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "test commit"]);

    let test_cases = vec![Some(0), Some(1), Some(usize::MAX)];
    for max_size in test_cases {
        let opts = fr::Options {
            source: repo.clone(),
            target: repo.clone(),
            refs: vec!["--all".to_string()],
            max_blob_size: max_size,
            ..Default::default()
        };
        let _result = fr::run(&opts);
    }
}

#[test]
fn error_handling_malformed_utf8_content() {
    let repo = init_repo();
    let malformed_utf8 = vec![0x66, 0x69, 0x6c, 0x65, 0x80, 0x81, 0x2e, 0x74, 0x78, 0x74];
    std::fs::write(repo.join("test.bin"), malformed_utf8).unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add malformed utf8"]);

    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        ..Default::default()
    };
    let _result = fr::run(&opts);
}

#[test]
fn error_handling_permission_denied_simulation() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "test commit"]);

    let readonly_dir = repo.join("readonly");
    std::fs::create_dir_all(&readonly_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&readonly_dir).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&readonly_dir, perms).unwrap();
    }

    let opts = fr::Options {
        source: repo.clone(),
        target: readonly_dir.clone(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        ..Default::default()
    };
    let _result = fr::run(&opts);
}

#[test]
fn error_handling_corrupted_git_repository() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let repo_path = temp_dir.path();
    run_git(&repo_path, &["init"]);
    let git_dir = repo_path.join(".git");
    let objects_dir = git_dir.join("objects");
    if objects_dir.exists() {
        std::fs::remove_dir_all(&objects_dir).unwrap();
    }
    let opts = fr::Options {
        source: repo_path.to_path_buf(),
        target: repo_path.to_path_buf(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        ..Default::default()
    };
    let result = fr::run(&opts);
    assert!(result.is_err());
}

#[test]
fn error_handling_extremely_long_paths() {
    let repo = init_repo();
    let long_filename = "a".repeat(200);
    let long_path = repo.join(&long_filename);
    std::fs::write(&long_path, "content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "add long filename"]);
    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        paths: vec![long_filename.into_bytes()],
        ..Default::default()
    };
    let _result = fr::run(&opts);
}

