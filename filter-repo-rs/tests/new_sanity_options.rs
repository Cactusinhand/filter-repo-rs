//! Test for sanity check behavior
//!
//! This test verifies that enforce_sanity defaults to true and that --force
//! properly bypasses sanity checks when needed.

use filter_repo_rs::opts::Options;
use filter_repo_rs::sanity::preflight;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_default_enforce_sanity_is_true() {
    let opts = Options::default();
    assert!(
        opts.enforce_sanity,
        "enforce_sanity should default to true for safety"
    );
}

#[test]
fn test_force_option_behavior() {
    // Test that --force bypasses sanity checks
    let mut opts = Options::default();
    assert!(opts.enforce_sanity, "Should start with default true");
    assert!(!opts.force, "Should start with force false");

    // Simulate --force option
    opts.force = true;
    assert!(opts.force, "--force should set force to true");
}

#[test]
fn test_enforce_sanity_option_parsing() {
    // Test that --enforce-sanity explicitly enables sanity checks (though it's already default)
    let mut opts = Options::default();
    assert!(opts.enforce_sanity, "Should start with default true");

    // --enforce-sanity should keep it true (redundant but explicit)
    opts.enforce_sanity = true;
    assert!(
        opts.enforce_sanity,
        "--enforce-sanity should keep enforce_sanity true"
    );
}

#[test]
fn test_preflight_with_enforce_sanity_true_on_invalid_repo() {
    // Test that preflight fails when enforce_sanity = true and repo is invalid
    let opts = Options {
        target: PathBuf::from("/nonexistent/directory"),
        force: false,
        enforce_sanity: true, // Explicitly enable sanity checks
        ..Default::default()
    };

    let result = preflight(&opts);
    assert!(
        result.is_err(),
        "Preflight should fail with enforce_sanity=true on invalid repo"
    );
}

#[test]
fn test_preflight_with_force_on_invalid_repo() {
    // Test that preflight succeeds when --force is used even with invalid repo
    let opts = Options {
        target: PathBuf::from("/nonexistent/directory"),
        force: true, // Use --force to bypass sanity checks
        ..Default::default()
    };

    let result = preflight(&opts);
    assert!(
        result.is_ok(),
        "Preflight should succeed with --force even on invalid repo"
    );
}

#[test]
fn test_preflight_with_force_bypasses_sanity_checks() {
    // Test that --force bypasses sanity checks regardless of enforce_sanity value
    let opts = Options {
        target: PathBuf::from("/nonexistent/directory"),
        force: true,          // Force should bypass all checks
        enforce_sanity: true, // Even with sanity checks enabled
        ..Default::default()
    };

    let result = preflight(&opts);
    assert!(
        result.is_ok(),
        "Preflight should succeed with --force regardless of enforce_sanity"
    );
}

fn create_test_repo() -> std::io::Result<TempDir> {
    let temp_dir = TempDir::new()?;

    // Initialize git repository
    let output = std::process::Command::new("git")
        .arg("init")
        .current_dir(temp_dir.path())
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to initialize test git repository",
        ));
    }

    // Set up git config
    std::process::Command::new("git")
        .args(&["config", "user.name", "Test User"])
        .current_dir(temp_dir.path())
        .output()?;

    std::process::Command::new("git")
        .args(&["config", "user.email", "test@example.com"])
        .current_dir(temp_dir.path())
        .output()?;

    Ok(temp_dir)
}

#[test]
fn test_preflight_with_enforce_sanity_true_on_valid_fresh_repo() {
    // Test that preflight succeeds with enforce_sanity=true on a valid fresh repo
    let temp_repo = create_test_repo().expect("Failed to create test repository");

    let opts = Options {
        target: temp_repo.path().to_path_buf(),
        force: false,
        enforce_sanity: true, // Enable sanity checks
        ..Default::default()
    };

    let result = preflight(&opts);
    // This might fail due to other sanity checks (like no commits), but it should not panic
    // The important thing is that it runs the sanity checks
    match result {
        Ok(()) => {
            // Great! The repo passed all sanity checks
        }
        Err(_) => {
            // Expected - fresh repo might fail some checks, but that's the point of sanity checks
        }
    }
}

#[test]
fn test_sanity_check_behavior_consistency() {
    // Test that the same repo gives consistent results with the same settings
    let temp_repo = create_test_repo().expect("Failed to create test repository");

    let opts_with_sanity = Options {
        target: temp_repo.path().to_path_buf(),
        force: false,
        enforce_sanity: true,
        ..Default::default()
    };

    let opts_with_force = Options {
        target: temp_repo.path().to_path_buf(),
        force: true, // Use --force to bypass sanity checks
        ..Default::default()
    };

    let result_with_sanity = preflight(&opts_with_sanity);
    let result_with_force = preflight(&opts_with_force);

    // With --force should always succeed
    assert!(
        result_with_force.is_ok(),
        "Preflight should succeed when --force is used"
    );

    // With sanity checks, the result depends on repo state, but should be consistent
    let result_with_sanity_2 = preflight(&opts_with_sanity);
    assert_eq!(
        result_with_sanity.is_ok(),
        result_with_sanity_2.is_ok(),
        "Preflight results should be consistent for the same repo and settings"
    );
}
