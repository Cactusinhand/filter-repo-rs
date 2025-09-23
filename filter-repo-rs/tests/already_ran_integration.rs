use std::fs;
use std::process::Command;
use tempfile::TempDir;

use filter_repo_rs::FilterRepoError;

fn create_test_repo() -> Result<TempDir, FilterRepoError> {
    let temp_dir = TempDir::new()?;

    // Initialize git repository
    let output = Command::new("git")
        .arg("init")
        .current_dir(temp_dir.path())
        .output()?;

    if !output.status.success() {
        return Err(FilterRepoError::from(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to initialize test git repository",
        )));
    }

    // Configure git user for commits
    let output = Command::new("git")
        .arg("config")
        .arg("user.name")
        .arg("Test User")
        .current_dir(temp_dir.path())
        .output()?;
    if !output.status.success() {
        return Err(FilterRepoError::from(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to configure git user.name",
        )));
    }

    Command::new("git")
        .arg("config")
        .arg("user.email")
        .arg("test@example.com")
        .current_dir(temp_dir.path())
        .output()?;

    // Create a test file and commit
    fs::write(temp_dir.path().join("test.txt"), "test content")?;

    Command::new("git")
        .arg("add")
        .arg("test.txt")
        .current_dir(temp_dir.path())
        .output()?;

    Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("Test commit")
        .current_dir(temp_dir.path())
        .output()?;

    Ok(temp_dir)
}

#[test]
fn test_already_ran_detection_integration() -> Result<(), FilterRepoError> {
    let temp_repo = create_test_repo()?;
    let repo_path = temp_repo.path();

    // Test 1: Fresh repository should create marker file
    let checker = filter_repo_rs::sanity::AlreadyRanChecker::new(repo_path)?;
    let state = checker.check_already_ran()?;
    assert!(matches!(
        state,
        filter_repo_rs::sanity::AlreadyRanState::NotRan
    ));

    // Mark as ran
    checker.mark_as_ran()?;

    // Test 2: Recent run should be detected
    let state = checker.check_already_ran()?;
    assert!(matches!(
        state,
        filter_repo_rs::sanity::AlreadyRanState::RecentRan
    ));

    // Test 3: Verify marker file exists
    let marker_file = repo_path.join(".git/filter-repo/already_ran");
    assert!(marker_file.exists(), "Marker file should exist");

    // Test 4: Clear marker and verify
    checker.clear_ran_marker()?;
    assert!(!marker_file.exists(), "Marker file should be removed");
    let state = checker.check_already_ran()?;
    assert!(matches!(
        state,
        filter_repo_rs::sanity::AlreadyRanState::NotRan
    ));

    Ok(())
}

#[test]
fn test_already_ran_detection_with_preflight() -> Result<(), FilterRepoError> {
    let temp_repo = create_test_repo()?;

    let opts = filter_repo_rs::Options {
        target: temp_repo.path().to_path_buf(),
        force: false,
        enforce_sanity: true,
        ..Default::default()
    };

    // First run should create the marker file (may fail on other sanity checks but should pass already ran detection)
    let _result = filter_repo_rs::sanity::preflight(&opts);

    // Verify the already_ran file was created
    let checker = filter_repo_rs::sanity::AlreadyRanChecker::new(temp_repo.path())?;
    assert!(checker.marker_file_exists());

    Ok(())
}
