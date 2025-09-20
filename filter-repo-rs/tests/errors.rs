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

    let result = fr::run(&opts);
    assert!(result.is_err(), "expected invalid SHA list to error");
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
    let result = fr::run(&opts);
    assert!(
        result.is_err(),
        "path rename with identical paths should fail"
    );
}

#[test]
fn error_handling_invalid_max_blob_size_values() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "test commit"]);

    let test_cases = vec![Some(0), Some(usize::MAX)];
    for max_size in test_cases {
        let opts = fr::Options {
            source: repo.clone(),
            target: repo.clone(),
            refs: vec!["--all".to_string()],
            max_blob_size: max_size,
            ..Default::default()
        };
        let result = fr::run(&opts);
        assert!(
            result.is_err(),
            "max_blob_size {:?} should be rejected",
            max_size
        );
    }
}

#[test]
fn error_handling_invalid_regex_pattern() {
    let repo = init_repo();
    let invalid_regex_file = repo.join("invalid_regex.txt");
    std::fs::write(&invalid_regex_file, b"regex:(?P<unterminated").unwrap();

    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        replace_text_file: Some(invalid_regex_file),
        ..Default::default()
    };
    let result = fr::run(&opts);
    assert!(result.is_err());
    let msg = format!("{:?}", result.err().unwrap());
    assert!(msg.contains("invalid regex"));
}

#[test]
fn error_handling_permission_denied_simulation() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "test commit"]);

    let restricted_dir = repo.join("restricted");
    std::fs::create_dir_all(&restricted_dir).unwrap();

    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        replace_text_file: Some(restricted_dir.clone()),
        ..Default::default()
    };
    let result = fr::run(&opts);
    assert!(result.is_err());
    let msg = format!("{:?}", result.err().unwrap());
    assert!(msg.contains("failed to read --replace-text"));
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
    let long_path_entry = vec![b'a'; 5000];
    let opts = fr::Options {
        source: repo.clone(),
        target: repo.clone(),
        refs: vec!["--all".to_string()],
        max_blob_size: Some(1000),
        paths: vec![long_path_entry],
        ..Default::default()
    };
    let result = fr::run(&opts);
    assert!(
        result.is_err(),
        "expected extremely long paths to trigger an error"
    );
}
